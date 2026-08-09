#!/bin/bash

cargo build --release
sudo ./target/release/drone-vtx --mode rx --interface wlan0