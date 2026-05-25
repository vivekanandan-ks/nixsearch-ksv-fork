{
  perSystem =
    {
      pkgs,
      lib,
      self',
      ...
    }:
    let
      baseEnv = [
        "NIX_SEARCH_SERVER__LISTEN=0.0.0.0:3000"
        "NIX_SEARCH_DATA__ARTIFACT_URL=file:///data/artifacts"
        "NIX_SEARCH_DATA__INDEX_DIR=/data/indexes"
      ];

      imageConfig = extraEnv: {
        Entrypoint = [ (lib.getExe self'.packages.cli) ];
        Cmd = [ "serve" ];

        Env = baseEnv ++ extraEnv;

        ExposedPorts = {
          "3000/tcp" = { };
        };

        Volumes = {
          "/data" = { };
        };
      };

      mkContainer =
        {
          name,
          contents,
          extraEnv ? [ ],
        }:
        pkgs.dockerTools.buildLayeredImage {
          inherit name contents;
          tag = "latest";
          maxLayers = 120;

          config = imageConfig extraEnv;
        };
    in
    {
      packages = lib.optionalAttrs pkgs.stdenv.isLinux {
        container = mkContainer {
          name = "nix-search";
          contents = [
            self'.packages.cli
            pkgs.cacert
          ];
          extraEnv = [
            "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          ];
        };

        containerWithNix = mkContainer {
          name = "nix-search-with-nix";
          contents = [
            self'.packages.cli
            pkgs.cacert
            pkgs.nix
          ];
          extraEnv = [
            "NIX_PATH=nixpkgs=${pkgs.path}"
            "NIX_CONFIG=experimental-features = nix-command flakes\nbuild-users-group ="
            "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          ];
        };
      };
    };
}
