#!/bin/bash

wget http://prdownloads.sourceforge.net/ta-lib/ta-lib-0.4.0-src.tar.gz || {
  echo "Download failed"
  exit 1
}
tar -xzf ta-lib-0.4.0-src.tar.gz || {
  echo "Extraction failed"
  exit 1
}

cd ta-lib || {
  echo "Cannot cd ta-lib"
  exit 1
}
./configure --prefix=/usr || {
  echo "Configure failed"
  exit 1
}
make || {
  echo "Make failed"
  exit 1
}
sudo make install || {
  echo "Install failed"
  exit 1
}

cd ..
rm -rf ta-lib ta-lib-0.4.0-src.tar.gz
echo "TA-Lib installed successfully"
