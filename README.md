# ztop

Display ZFS datasets' I/O in real time

[![Build Status](https://img.shields.io/github/actions/workflow/status/asomers/ztop/ci.yml?branch=master&style=flat-square)](https://github.com/asomers/ztop/actions/workflows/ci.yml?query=branch%3Amaster)
[![Crates.io](https://img.shields.io/crates/v/ztop.svg)](https://crates.io/crates/ztop)

# Overview

`ztop` is like `top`, but for ZFS datasets.  It displays the real-time activity
for datasets.  The built-in `zpool iostat` can display real-time I/O statistics
for pools, but until now there was no similar tool for datasets.

# Platform support

`ztop` works on FreeBSD 12 and later, and Linux.

# Screenshot

![Screenshot 1](https://raw.githubusercontent.com/asomers/ztop/master/doc/demo.gif)

# Minimum Supported Rust Version (MSRV)

ztop does not guarantee any specific MSRV.  Rather, it guarantees compatibility
with the oldest rustc shipped in the package collection of each supported
operating system.

* https://www.freshports.org/lang/rust/

# License

`ztop` is primarily distributed under the terms of the BSD 2-clause license.

See LICENSE for details.

# Sponsorship

ztop is sponsored by Axcient, inc.
