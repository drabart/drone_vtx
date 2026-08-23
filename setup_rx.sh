#!/bin/bash
set -e

sudo ip link set wlp11s0u5u3 down
sudo iw dev wlp11s0u5u3 set type monitor
sudo iw dev wlp11s0u5u3 set txpower fixed 2000
sudo ip link set wlp11s0u5u3 up
sudo iw dev wlp11s0u5u3 set channel 36 HT20
