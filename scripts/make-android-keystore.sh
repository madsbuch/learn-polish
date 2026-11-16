#!/usr/bin/env bash
# Creates the signing key for the Android APK and prints the four GitHub secrets
# to paste into Settings → Secrets and variables → Actions.
#
# Run this once. Keep upgrade-key.jks somewhere safe — if you lose it, you can no
# longer publish an APK that upgrades an already-installed copy of the app.
set -euo pipefail

OUT="${1:-upgrade-key.jks}"
ALIAS="${ALIAS:-polski}"

if [[ -e "$OUT" ]]; then
  echo "Refusing to overwrite existing $OUT" >&2
  exit 1
fi

read -rsp "Choose a keystore password: " PASSWORD
echo
if [[ ${#PASSWORD} -lt 6 ]]; then
  echo "keytool requires at least 6 characters." >&2
  exit 1
fi

keytool -genkeypair \
  -keystore "$OUT" \
  -alias "$ALIAS" \
  -keyalg RSA \
  -keysize 4096 \
  -validity 10000 \
  -storepass "$PASSWORD" \
  -keypass "$PASSWORD" \
  -dname "CN=Polski, OU=Personal, O=Polski, C=DK"

cat <<INFO

Created $OUT

Add these four repository secrets:

  ANDROID_KEYSTORE_BASE64
$(base64 -w0 "$OUT" 2>/dev/null || base64 "$OUT" | tr -d '\n')

  ANDROID_KEYSTORE_PASSWORD   $PASSWORD
  ANDROID_KEY_ALIAS           $ALIAS
  ANDROID_KEY_PASSWORD        $PASSWORD

Release builds require these secrets and fail if any are missing.
Pull requests build unsigned test APKs without accessing the release key.
INFO
