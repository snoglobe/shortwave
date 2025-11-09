#!/bin/bash
# Quick start script for Shortwave web player

echo "Starting Shortwave Web Player..."
echo ""
echo "Make sure a Shortwave node is running on http://localhost:8080"
echo ""

cd "$(dirname "$0")"
npm run dev

