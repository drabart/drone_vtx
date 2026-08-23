#!/bin/bash
set -e

cargo build --release
sudo -E DISPLAY=$DISPLAY XAUTHORITY=$XAUTHORITY ./target/release/drone-vtx --mode rx --interface wlp11s0u5u3