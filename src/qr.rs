//! Wi-Fi QR payloads and terminal rendering.
//!
//! The payload follows the common `WIFI:T:...;S:...;P:...;;` convention used by
//! Android and iOS camera apps. Modules are rendered with half blocks so each
//! terminal cell contains two vertically stacked QR modules.

use qrcode::types::Color as QrColor;
use qrcode::{EcLevel, QrCode};

use crate::backend::Security;

pub fn wifi_payload(
    ssid: &str,
    security: Security,
    password: Option<&str>,
    hidden: bool,
) -> Result<String, String> {
    let qr_type = security.qr_type().ok_or_else(|| {
        format!(
            "{} networks cannot be encoded as a password-only Wi-Fi QR code",
            security.label()
        )
    })?;

    let mut payload = format!("WIFI:T:{qr_type};S:{};", escape(ssid));
    if security.needs_password() {
        let password = password
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "No saved password is available for this network".to_string())?;
        payload.push_str(&format!("P:{};", escape(password)));
    }
    if hidden {
        payload.push_str("H:true;");
    }
    payload.push(';');
    Ok(payload)
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | ';' | ',' | ':' | '"') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

pub struct QrView {
    pub ssid: String,
    pub security_label: String,
    pub size: usize,
    modules: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QrLayout {
    pub quiet: usize,
    pub width: u16,
    pub height: u16,
    pub footer: bool,
}

impl QrView {
    pub fn new(payload: String, ssid: String, security_label: String) -> Result<Self, String> {
        let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M)
            .or_else(|_| QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::L))
            .map_err(|error| format!("Could not encode QR code: {error}"))?;
        let size = code.width();
        let modules = code
            .into_colors()
            .into_iter()
            .map(|color| color == QrColor::Dark)
            .collect();

        Ok(Self {
            ssid,
            security_label,
            size,
            modules,
        })
    }

    fn dark(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 || x >= self.size as isize || y >= self.size as isize {
            return false;
        }
        self.modules[y as usize * self.size + x as usize]
    }

    pub fn lines(&self, quiet: usize) -> Vec<String> {
        let full = self.size + quiet * 2;
        let mut lines = Vec::with_capacity(full.div_ceil(2));
        let mut y = 0;
        while y < full {
            let mut line = String::with_capacity(full);
            for x in 0..full {
                let top = self.dark(x as isize - quiet as isize, y as isize - quiet as isize);
                let bottom = self.dark(
                    x as isize - quiet as isize,
                    (y + 1) as isize - quiet as isize,
                );
                line.push(match (top, bottom) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                });
            }
            lines.push(line);
            y += 2;
        }
        lines
    }

    /// Choose the largest standard quiet zone that fits, falling back to a
    /// compact zone when the terminal is short.
    pub fn layout(&self, width: u16, height: u16) -> Option<QrLayout> {
        for quiet in [4usize, 2, 1] {
            let qr_width = self.size + quiet * 2;
            let qr_height = qr_width.div_ceil(2);
            if qr_width > width as usize {
                continue;
            }
            if qr_height + 2 <= height as usize {
                return Some(QrLayout {
                    quiet,
                    width: qr_width as u16,
                    height: qr_height as u16,
                    footer: true,
                });
            }
            if qr_height < height as usize {
                return Some(QrLayout {
                    quiet,
                    width: qr_width as u16,
                    height: qr_height as u16,
                    footer: false,
                });
            }
        }
        None
    }

    pub fn minimum_size(&self) -> (u16, u16) {
        let width = self.size + 2;
        (width as u16, width.div_ceil(2) as u16 + 1)
    }

    #[cfg(test)]
    fn pbm(&self, quiet: usize) -> String {
        let full = self.size + quiet * 2;
        let mut output = format!("P1\n{full} {full}\n");
        for y in 0..full {
            for x in 0..full {
                let dark = self.dark(x as isize - quiet as isize, y as isize - quiet as isize);
                output.push(if dark { '1' } else { '0' });
                output.push(' ');
            }
            output.push('\n');
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_escapes_special_characters() {
        assert_eq!(
            wifi_payload("Cafe;Guest", Security::Wpa, Some("pa:ss,word"), false).unwrap(),
            r"WIFI:T:WPA;S:Cafe\;Guest;P:pa\:ss\,word;;"
        );
    }

    #[test]
    fn open_network_omits_password() {
        assert_eq!(
            wifi_payload("Cafe", Security::Open, None, false).unwrap(),
            "WIFI:T:nopass;S:Cafe;;"
        );
    }

    #[test]
    fn hidden_network_is_marked() {
        assert_eq!(
            wifi_payload("Cafe", Security::Wpa, Some("secret"), true).unwrap(),
            "WIFI:T:WPA;S:Cafe;P:secret;H:true;;"
        );
    }

    #[test]
    fn enterprise_networks_are_rejected() {
        assert!(wifi_payload("Corp", Security::Enterprise, Some("secret"), false).is_err());
    }

    #[test]
    fn qr_lines_are_square_and_include_quiet_zone() {
        let view = QrView::new(
            "WIFI:T:nopass;S:Cafe;;".to_string(),
            "Cafe".to_string(),
            "Open".to_string(),
        )
        .unwrap();
        let lines = view.lines(4);
        assert_eq!(lines.len(), (view.size + 8).div_ceil(2));
        assert!(
            lines
                .iter()
                .all(|line| line.chars().count() == view.size + 8)
        );
        assert!(lines.iter().any(|line| line.contains('█')));
        assert!(lines.iter().all(|line| {
            line.chars()
                .all(|character| matches!(character, '█' | '▀' | '▄' | ' '))
        }));
    }

    #[test]
    #[ignore = "requires zbarimg to be installed"]
    fn qr_matrix_decodes_to_the_wifi_payload() {
        use std::process::Command;

        let payload = r"WIFI:T:WPA;S:Cafe\;Guest;P:pa\:ss;;";
        let view = QrView::new(
            payload.to_string(),
            "Cafe;Guest".to_string(),
            "WPA".to_string(),
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "bnm-qr-{}-{}.pbm",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, view.pbm(4)).unwrap();
        let png_path = path.with_extension("png");
        let convert = Command::new("convert")
            .arg(&path)
            .args(["-scale", "800%"])
            .arg(&png_path)
            .status()
            .expect("ImageMagick convert should be installed");
        assert!(convert.success(), "could not convert PBM to PNG");
        let output = Command::new("zbarimg")
            .args(["--quiet", "--raw"])
            .arg(&png_path)
            .output()
            .expect("zbarimg should be installed");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&png_path);
        assert!(
            output.status.success(),
            "zbarimg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), payload);
    }

    #[test]
    fn layout_prefers_larger_quiet_zone_when_possible() {
        let view = QrView::new(
            "WIFI:T:nopass;S:Cafe;;".to_string(),
            "Cafe".to_string(),
            "Open".to_string(),
        )
        .unwrap();
        let layout = view.layout(200, 80).unwrap();
        assert_eq!(layout.quiet, 4);
        assert!(view.layout(10, 5).is_none());
    }
}
