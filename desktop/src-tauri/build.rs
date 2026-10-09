// Generate the deterministic bootstrap PNG before tauri-build. The final
// product artwork is a separate release-qualification requirement.
const ICON: &str = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAABbklEQVR42u2b0RGDIBBE5SZVJR0k7cQqkna0A20r+YofjIgi3F3ulpl8Au7uO8AMdp3zFko73q+vjyYh49wHFgO0CT9rRLAivNSIYE34USPIsvg9Gsiy+D1ayLr4nCbyIH5LG3k/CJGX9FMaQYCn9Ne0ggBv6ccUgAAYAAPqtWF6sjx0zXku/yR8bc7H7X1qnFC6C2yJPvtQXPOOcx+wBpR2bJky5/MQN6baxkUJSGCnBf+mBNTGtVVZoQQk8Rum5/KTKsOmBKSErYneMqLlSRMlwL0K59I8knaNXQgEcL667k23pI+4AdyHolrzuS+BC8ckJRhz/cmCRVC6Lrn6gACNBvySlHytplaitG9/4gTEQqQowBrwD5i2HJc04C9ZBigBbSlz0wECNG51nFvkcpPa2z2h3y1ylEDsiKf0QUBsgAcKYo0gIOeQ5fSTBFg0IaWJjnawJD67BlgwIacBH04eHdjtp7PajfB0kq3aviU3sRKUrQoEAAAAAElFTkSuQmCC";

fn decode_base64(input: &str) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a' + 26),
            b'0'..=b'9' => u32::from(byte - b'0' + 52),
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => panic!("unexpected base64 byte"),
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    output
}

fn main() {
    let icon = std::path::Path::new("icons/icon.png");
    if !icon.exists() {
        std::fs::create_dir_all("icons").expect("create bootstrap icon directory");
        std::fs::write(icon, decode_base64(ICON)).expect("write bootstrap icon");
    }
    tauri_build::build();
}
