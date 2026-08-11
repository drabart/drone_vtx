#!/bin/bash

cargo build --release
sudo -E DISPLAY=$DISPLAY XAUTHORITY=$XAUTHORITY ./target/release/drone-vtx --mode rx --interface wlan0