# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - ReleaseDate

### Fixed

- Don't crash if two different pools have objsets of the same ID that list
  adjacently in the sysctl tree.
  (#[15](https://github.com/asomers/ztop/pull/15))

## [0.1.1] - 2021-08-13

### Fixed

- Don't crash on FreeBSD 12.2
  (#[5](https://github.com/asomers/ztop/pull/5))

- Don't crash if no datasets are present
  (#[6](https://github.com/asomers/ztop/pull/6))
