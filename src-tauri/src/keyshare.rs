//! Moving the API keys between devices with a QR code.
//!
//! One device shows its keys as a QR code (rendered here, so the keys reach
//! the webview only as a picture); the other scans it and hands the text back
//! to `import`, which saves whichever keys it carries.

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use serde::{Deserialize, Serialize};

use crate::db::{self, Conn};
use crate::error::{Error, Result};

/// Marks a QR code as Polski's, and its format version.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Payload {
    polski: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    openrouter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kie: Option<String>,
}

fn saved(conn: &mut Conn, key: &str) -> Result<Option<String>> {
    Ok(db::get_setting(conn, key)?
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

/// The keys saved on this device, as the text a QR code carries.
fn payload(conn: &mut Conn) -> Result<String> {
    let payload = Payload {
        polski: VERSION,
        openrouter: saved(conn, "openrouter_api_key")?,
        kie: saved(conn, "kie_api_key")?,
    };
    if payload.openrouter.is_none() && payload.kie.is_none() {
        return Err(Error::msg("There are no API keys on this device to share."));
    }
    Ok(serde_json::to_string(&payload)?)
}

/// The keys saved on this device as a QR code, in SVG.
pub fn qr_svg(conn: &mut Conn) -> Result<String> {
    let text = payload(conn)?;
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)
        .map_err(|e| Error::msg(format!("Could not make the QR code: {e}")))?;
    // Black on white whatever the theme: scanners need the contrast.
    Ok(code
        .render::<svg::Color>()
        .quiet_zone(true)
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

/// Reads a scanned QR code. Errors when it is not one of Polski's.
fn parse(text: &str) -> Result<(Option<String>, Option<String>)> {
    let not_ours = || Error::msg("That QR code does not hold Polski API keys.");
    let payload: Payload = serde_json::from_str(text.trim()).map_err(|_| not_ours())?;
    if payload.polski != VERSION {
        return Err(Error::msg(
            "That QR code comes from a different version of Polski. Update both devices and try again.",
        ));
    }
    let clean = |key: Option<String>| key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    let keys = (clean(payload.openrouter), clean(payload.kie));
    if keys.0.is_none() && keys.1.is_none() {
        return Err(not_ours());
    }
    Ok(keys)
}

/// Saves the keys a scanned QR code carries. Keys it does not carry are left alone.
pub fn import(conn: &mut Conn, text: &str) -> Result<()> {
    let (openrouter, kie) = parse(text)?;
    if let Some(key) = openrouter {
        db::set_setting(conn, "openrouter_api_key", &key)?;
    }
    if let Some(key) = kie {
        db::set_setting(conn, "kie_api_key", &key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_it_writes() {
        let text = serde_json::to_string(&Payload {
            polski: VERSION,
            openrouter: Some("sk-or-v1-abc".into()),
            kie: None,
        })
        .unwrap();
        assert_eq!(parse(&text).unwrap(), (Some("sk-or-v1-abc".into()), None));
    }

    #[test]
    fn rejects_other_codes() {
        assert!(parse("https://example.com").is_err());
        assert!(parse(r#"{"polski":1}"#).is_err());
        assert!(parse(r#"{"polski":1,"openrouter":"  "}"#).is_err());
        assert!(parse(r#"{"polski":2,"openrouter":"x"}"#).is_err());
    }

    #[test]
    fn renders_a_qr_code() {
        let code = QrCode::new(b"{\"polski\":1,\"openrouter\":\"sk-or-v1-0123456789abcdef\"}").unwrap();
        let svg = code.render::<svg::Color>().build();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
    }
}
