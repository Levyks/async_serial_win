use windows_sys::Win32::System::Registry::*;

/// Lists ports via HKLM\HARDWARE\DEVICEMAP\SERIALCOMM. This is the simple,
/// fast enumeration path (path + Windows value name only). Richer metadata
/// (friendly name, hardware/VID:PID) via SetupAPI can be layered on later
/// without changing the public Dart API.
pub fn list_ports_json() -> String {
    let mut entries: Vec<(String, String)> = Vec::new();

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let subkey: Vec<u16> = "HARDWARE\\DEVICEMAP\\SERIALCOMM\0".encode_utf16().collect();
        let status = RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey);
        if status != 0 {
            return "[]".to_string();
        }

        let mut index = 0u32;
        loop {
            let mut name_buf = [0u16; 256];
            let mut name_len = name_buf.len() as u32;
            let mut value_buf = [0u8; 512];
            let mut value_len = value_buf.len() as u32;
            let mut value_type: u32 = 0;

            let status = RegEnumValueW(
                hkey,
                index,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                &mut value_type,
                value_buf.as_mut_ptr(),
                &mut value_len,
            );

            if status != 0 {
                break;
            }

            // value is a REG_SZ wide string.
            let wide_len = (value_len as usize) / 2;
            let wide_slice = std::slice::from_raw_parts(value_buf.as_ptr() as *const u16, wide_len);
            let mut path = String::from_utf16_lossy(wide_slice);
            path = path.trim_end_matches('\0').to_string();

            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);

            if !path.is_empty() {
                entries.push((path, name));
            }
            index += 1;
        }

        windows_sys::Win32::System::Registry::RegCloseKey(hkey);
    }

    let mut json = String::from("[");
    for (i, (path, name)) in entries.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            "{{\"path\":{},\"name\":{}}}",
            json_string(path),
            json_string(name)
        ));
    }
    json.push(']');
    json
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(json_string(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn escapes_newline_and_carriage_return() {
        assert_eq!(json_string("a\nb\rc"), r#""a\nb\rc""#);
    }

    #[test]
    fn escapes_other_control_characters_as_unicode_escapes() {
        assert_eq!(json_string("a\tb"), "\"a\\u0009b\"");
    }

    #[test]
    fn leaves_plain_ascii_untouched() {
        assert_eq!(json_string("COM3"), "\"COM3\"");
    }

    #[test]
    fn list_ports_json_is_always_a_valid_json_array() {
        // Doesn't assert on port contents (machine-dependent), just that the
        // registry-scanning path always produces parseable JSON.
        let json = list_ports_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("list_ports_json must produce valid JSON");
        assert!(parsed.is_array());
    }
}
