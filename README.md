# ztop

Display ZFS datasets' I/O in real time

[![Build Status](https://api.cirrus-ci.com/github/asomers/ztop.svg)](https://cirrus-ci.com/github/asomers/ztop)
[![Crates.io](https://img.shields.io/crates/v/ztop.svg)](https://crates.io/crates/ztop)

# Overview

`ztop` is like `top`, but for ZFS datasets.  It displays the real-time activity
for datasets.  The built-in `zpool iostat` can display real-time I/O statistics
for pools, but until now there was no similar tool for datasets.

# Platform support

`ztop` works on FreeBSD 12 and later.  It would probably work on Linux with
minor modifications.  Patches welcome.

# Screenshot

![Screenshot 1](https://raw.githubusercontent.com/asomers/ztop/master/doc/demo.gif)

# Minimum Supported Rust Version (MSRV)

ztop is supported on Rust 1.53.0 and higher.  It's MSRV will not be
changed in the future without bumping the major or minor version.

# License

`ztop` is primarily distributed under the terms of the BSD 2-clause license.

See LICENSE for details.

# Sponsorship

ztop is sponsored by Axcient, inc.
