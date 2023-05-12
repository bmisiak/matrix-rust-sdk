#!/usr/bin/env bash
set -eEu

cd "$(dirname "$0")"

# Path to the repo root
SRC_ROOT=../..

TARGET_DIR="${SRC_ROOT}/target"

GENERATED_DIR="${SRC_ROOT}/generated"
if [ -d "${GENERATED_DIR}" ]; then rm -rf "${GENERATED_DIR}"; fi
mkdir -p ${GENERATED_DIR}/{macos,simulator,catalyst}

REL_FLAG="--release"
REL_TYPE_DIR="release"

TARGET_CRATE=matrix-sdk-crypto-ffi

# Build static libs for all the different architectures

# Required by olm-sys crate. @todo: Is this the right SDK path for Catalyst as well?
export IOS_SDK_PATH=`xcrun --show-sdk-path --sdk iphoneos`

# iOS
echo -e "Building for iOS [1/7]"
cargo build -p ${TARGET_CRATE} ${REL_FLAG} --target "aarch64-apple-ios"

# Mac Catalyst
# 1) The target has no pre=built rust-std, so we use `-Z build-std`.
#   how to build-std: https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#build-std
#   list of targets with prebuilt rust-std: https://doc.rust-lang.org/nightly/rustc/platform-support.html
# 2) requires a recent nightly wih https://github.com/rust-lang/rust/pull/111384 merged
CATALYST_RUST_NIGHTLY="nightly"
echo -e "\nInstalling nightly and rust-src for Mac Catalyst"
rustup toolchain install ${CATALYST_RUST_NIGHTLY} --no-self-update
rustup component add rust-src --toolchain ${CATALYST_RUST_NIGHTLY}

echo -e "\nBuilding for macOS Catalyst (Apple Silicon) [2/7]"
cargo +${CATALYST_RUST_NIGHTLY} build -p ${TARGET_CRATE} ${REL_FLAG} -Z build-std --target aarch64-apple-ios-macabi
echo -e "\nBuilding for macOS Catalyst (Intel) [3/7]"
cargo +${CATALYST_RUST_NIGHTLY} build -p ${TARGET_CRATE} ${REL_FLAG} -Z build-std --target x86_64-apple-ios-macabi

# MacOS
echo -e "\nBuilding for macOS (Apple Silicon) [4/7]"
cargo build -p ${TARGET_CRATE} ${REL_FLAG} --target "aarch64-apple-darwin"
echo -e "\nBuilding for macOS (Intel) [5/7]"
cargo build -p ${TARGET_CRATE} ${REL_FLAG} --target "x86_64-apple-darwin"

# iOS Simulator
echo -e "\nBuilding for iOS Simulator (Apple Silicon) [6/7]"
cargo build -p ${TARGET_CRATE} ${REL_FLAG} --target "aarch64-apple-ios-sim"
echo -e "\nBuilding for iOS Simulator (Intel) [7/7]"
cargo build -p ${TARGET_CRATE} ${REL_FLAG} --target "x86_64-apple-ios"

echo -e "\nCreating XCFramework"
# Lipo together the libraries for the same platform

# MacOS
lipo -create \
  "${TARGET_DIR}/x86_64-apple-darwin/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  "${TARGET_DIR}/aarch64-apple-darwin/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  -output "${GENERATED_DIR}/macos/libmatrix_sdk_crypto_ffi.a"

# Catalyst
lipo -create \
  "${TARGET_DIR}/aarch64-apple-ios-macabi/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  "${TARGET_DIR}/x86_64-apple-ios-macabi/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  -output "${GENERATED_DIR}/catalyst/libmatrix_sdk_crypto_ffi.a"

# iOS Simulator
lipo -create \
  "${TARGET_DIR}/x86_64-apple-ios/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  "${TARGET_DIR}/aarch64-apple-ios-sim/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  -output "${GENERATED_DIR}/simulator/libmatrix_sdk_crypto_ffi.a"

# Generate uniffi files
cargo uniffi-bindgen generate \
  --language swift \
  --lib-file "${TARGET_DIR}/aarch64-apple-ios-sim/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  --config "${SRC_ROOT}/bindings/${TARGET_CRATE}/uniffi.toml" \
  --out-dir ${GENERATED_DIR} \
  "${SRC_ROOT}/bindings/${TARGET_CRATE}/src/olm.udl"

# Move headers to the right place
HEADERS_DIR=${GENERATED_DIR}/headers
mkdir -p ${HEADERS_DIR}
mv ${GENERATED_DIR}/*.h ${HEADERS_DIR}

# Rename and move modulemap to the right place
mv ${GENERATED_DIR}/*.modulemap ${HEADERS_DIR}/module.modulemap

# Move source files to the right place
SWIFT_DIR="${GENERATED_DIR}/Sources"
mkdir -p ${SWIFT_DIR}
mv ${GENERATED_DIR}/*.swift ${SWIFT_DIR}

# Build the xcframework

if [ -d "${GENERATED_DIR}/MatrixSDKCryptoFFI.xcframework" ]; then rm -rf "${GENERATED_DIR}/MatrixSDKCryptoFFI.xcframework"; fi

xcodebuild -create-xcframework \
  -library "${TARGET_DIR}/aarch64-apple-ios/${REL_TYPE_DIR}/libmatrix_sdk_crypto_ffi.a" \
  -headers ${HEADERS_DIR} \
  -library "${GENERATED_DIR}/macos/libmatrix_sdk_crypto_ffi.a" \
  -headers ${HEADERS_DIR} \
  -library "${GENERATED_DIR}/simulator/libmatrix_sdk_crypto_ffi.a" \
  -headers ${HEADERS_DIR} \
  -library "${GENERATED_DIR}/catalyst/libmatrix_sdk_crypto_ffi.a" \
  -headers ${HEADERS_DIR} \
  -output "${GENERATED_DIR}/MatrixSDKCryptoFFI.xcframework"

# Cleanup
if [ -d "${GENERATED_DIR}/macos" ]; then rm -rf "${GENERATED_DIR}/macos"; fi
if [ -d "${GENERATED_DIR}/catalyst" ]; then rm -rf "${GENERATED_DIR}/catalyst"; fi
if [ -d "${GENERATED_DIR}/simulator" ]; then rm -rf "${GENERATED_DIR}/simulator"; fi
if [ -d ${HEADERS_DIR} ]; then rm -rf ${HEADERS_DIR}; fi

# Zip up framework, sources and LICENSE, ready to be uploaded to GitHub Releases and used by MatrixSDKCrypto.podspec
cp ${SRC_ROOT}/LICENSE $GENERATED_DIR
cd $GENERATED_DIR
zip -r MatrixSDKCryptoFFI.zip MatrixSDKCryptoFFI.xcframework Sources LICENSE
rm LICENSE

echo "XCFramework is ready 🚀"
