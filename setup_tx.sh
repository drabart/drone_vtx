#!/bin/bash

sudo ip link set wlan1 down
sudo iw dev wlan1 set type monitor
sudo iw dev wlan1 set txpower fixed 2000
sudo ip link set wlan1 up
sudo iw dev wlan1 set channel 36 HT20
