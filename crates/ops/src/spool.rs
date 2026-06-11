use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;

use nixsearch_core::document::SearchDocument;

pub(crate) struct DocumentSpool {
    _tempdir: tempfile::TempDir,
    path: Utf8PathBuf,
}

impl DocumentSpool {
    pub(crate) fn create() -> Result<Self> {
        let tempdir = tempfile::Builder::new()
            .prefix("nixsearch-generation-spool-")
            .tempdir()
            .context("failed to create generation document spool")?;
        let path = Utf8PathBuf::from_path_buf(tempdir.path().join("documents.jsonl"))
            .map_err(|path| anyhow::anyhow!("spool path is not UTF-8: {}", path.display()))?;

        Ok(Self {
            _tempdir: tempdir,
            path,
        })
    }

    pub(crate) fn writer(&self) -> Result<DocumentSpoolWriter> {
        let file = File::create(&self.path)
            .with_context(|| format!("failed to create document spool {}", self.path))?;

        Ok(DocumentSpoolWriter {
            writer: BufWriter::new(file),
        })
    }

    pub(crate) fn reader(&self) -> Result<DocumentSpoolReader> {
        let file = File::open(&self.path)
            .with_context(|| format!("failed to open document spool {}", self.path))?;

        Ok(DocumentSpoolReader {
            reader: BufReader::new(file),
            line_number: 0,
        })
    }
}

pub(crate) struct DocumentSpoolWriter {
    writer: BufWriter<File>,
}

impl DocumentSpoolWriter {
    pub(crate) fn push(&mut self, document: &SearchDocument) -> Result<()> {
        serde_json::to_writer(&mut self.writer, document)
            .context("failed to serialize document into spool")?;
        self.writer
            .write_all(b"\n")
            .context("failed to write document spool newline")?;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        self.writer
            .flush()
            .context("failed to flush document spool")
    }
}

pub(crate) struct DocumentSpoolReader {
    reader: BufReader<File>,
    line_number: usize,
}

impl Iterator for DocumentSpoolReader {
    type Item = Result<SearchDocument>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();

        match self.reader.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => {
                self.line_number += 1;
                let line = line.trim_end_matches(['\n', '\r']);
                Some(serde_json::from_str(line).with_context(|| {
                    format!(
                        "failed to deserialize document spool line {}",
                        self.line_number
                    )
                }))
            }
            Err(error) => Some(Err(error).context("failed to read document spool")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nixsearch_core::document::{PackageDoc, SearchDocument};
    use nixsearch_core::ingest::IngestContext;

    use super::DocumentSpool;

    fn package(name: &str) -> SearchDocument {
        SearchDocument::Package(PackageDoc::new(
            &IngestContext {
                source: "fixtures".to_owned(),
                ref_id: "small".to_owned(),
                revision: None,
                repo: None,
            },
            name,
        ))
    }

    #[test]
    fn spool_round_trips_documents() {
        let spool = DocumentSpool::create().unwrap();
        let documents = vec![package("git"), package("ripgrep")];

        let mut writer = spool.writer().unwrap();
        for document in &documents {
            writer.push(document).unwrap();
        }
        writer.finish().unwrap();

        let loaded = spool
            .reader()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(loaded, documents);
    }

    #[test]
    fn spool_reads_empty_file() {
        let spool = DocumentSpool::create().unwrap();
        spool.writer().unwrap().finish().unwrap();

        let loaded = spool
            .reader()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(loaded.is_empty());
    }

    #[test]
    fn spool_reports_invalid_json_line_number() {
        let spool = DocumentSpool::create().unwrap();
        fs::write(&spool.path, "{\"kind\":\"package\"}\nnot json\n").unwrap();

        let error = spool.reader().unwrap().nth(1).unwrap().unwrap_err();

        assert!(format!("{error:#}").contains("document spool line 2"));
    }
}
