# Code Guidelines

## General

Follow general code best practices:

- Use descriptive and well-chosen names for variables, functions, and classes.
- Avoid redundant duplication: if a string or a magic number is being duplicated multiple times, extract it to a single shared place.
- Avoid repeating yourself, such as with hard-coded strings or magic numbers; use constants or functions instead.
- Each function should do one 'job' and do it well.
- Avoid reinventing the wheel when a well-known library or tool can accomplish the task effectively.

## Python

Use uv always.

## Rust

Be eager about dependency usage rather than reinventing the wheel. Always search dependencies for the latest versions using cargo before recommending.

Structure imports:
use std::<whatever>
<new line>
use <dependency>::<whatever>
<new line>
use <crate name>::<whatever>

## Nix

Use flake-parts.
