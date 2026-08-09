#!/bin/bash

sudo ip link set wlan0 down
sudo iw dev wlan0 set type monitor
sudo iw dev wlan0 set txpower fixed 2000
sudo ip link set wlan0 up
sudo iw dev wlan0 set channel 36 HT20
