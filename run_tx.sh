#!/bin/bash

cargo build --release
sudo ./target/release/drone-vtx --mode tx --interface wlan1