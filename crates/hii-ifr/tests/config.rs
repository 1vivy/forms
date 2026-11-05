//! Behaviour of the configuration string helpers.

use hii_ifr::config::{self, Block, ConfigError};

#[test]
fn header_uses_lower_case_hex_and_efi_guid_order() {
    let guid = [
        0x2b, 0x7a, 0x24, 0x0d, 0xad, 0xd5, 0xd6, 0x4f, 0xbe, 0x1c, 0xdf, 0xa4, 0x41, 0x5f, 0x55,
        0x26,
    ];
    let text = config::header(&guid, "DtAcpiPref", &[0xde, 0xad]);
    assert_eq!(
        text,
        "GUID=2b7a240dadd5d64fbe1cdfa4415f5526&NAME=0044007400410063007000690050007200650066&PATH=dead"
    );
}

#[test]
fn empty_name_and_path_stay_empty() {
    assert_eq!(
        config::header(&[0; 16], "", &[]),
        format!("GUID={}&NAME=&PATH=", "00".repeat(16))
    );
}

#[test]
fn blocks_round_trip_through_a_response() {
    let header = config::header(&[0x11; 16], "Store", &[1, 2, 3]);
    let blocks = vec![
        Block {
            offset: 0,
            width: 1,
            bytes: vec![0x01],
        },
        Block {
            offset: 0x10,
            width: 3,
            bytes: vec![0xaa, 0xbb, 0xcc],
        },
    ];
    let response = config::response(&header, &blocks);
    assert_eq!(config::parse_blocks(&response).unwrap(), blocks);
}

#[test]
fn requests_round_trip_offsets() {
    let header = config::header(&[0; 16], "", &[]);
    let request = config::request(&header, &[(0, 2), (0x1f, 1)]);
    assert!(request.ends_with("&OFFSET=0&WIDTH=2&OFFSET=1f&WIDTH=1"));
    let response = format!("{header}&OFFSET=0&WIDTH=2&VALUE=0000&OFFSET=1f&WIDTH=1&VALUE=ff");
    assert_eq!(
        config::parse_blocks(&response).unwrap(),
        vec![
            Block {
                offset: 0,
                width: 2,
                bytes: vec![0, 0]
            },
            Block {
                offset: 0x1f,
                width: 1,
                bytes: vec![0xff]
            }
        ]
    );
}

#[test]
fn names_round_trip_with_a_header() {
    let header = config::header(&[0; 16], "", &[]);
    let request = config::request_names(&header, &["First", "Second"]);
    assert_eq!(request, format!("{header}&First&Second"));
    let response = format!("{header}&First=00&Second=0102");
    assert_eq!(
        config::parse_names(&response).unwrap(),
        vec![
            ("First".into(), "00".into()),
            ("Second".into(), "0102".into())
        ]
    );
}

#[test]
fn bodies_without_a_header_are_accepted() {
    assert_eq!(
        config::parse_blocks("&OFFSET=0&WIDTH=1&VALUE=01")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        config::parse_blocks("OFFSET=0&WIDTH=1&VALUE=01")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        config::parse_names("First=00").unwrap(),
        vec![("First".into(), "00".into())]
    );
    assert!(config::parse_blocks("").unwrap().is_empty());
}

#[test]
fn malformed_blocks_are_refused() {
    assert_eq!(
        config::parse_blocks("&OFFSET=0&WIDTH=2&VALUE=0"),
        Err(ConfigError::BadHex("VALUE"))
    );
    assert_eq!(
        config::parse_blocks("&OFFSET=0&WIDTH=3&VALUE=0123"),
        Err(ConfigError::WidthMismatch { width: 3, bytes: 2 })
    );
    assert!(matches!(
        config::parse_blocks("&WIDTH=1&VALUE=01"),
        Err(ConfigError::MissingField("OFFSET"))
    ));
    assert!(matches!(
        config::parse_blocks("&OFFSET=0&WIDTH=1"),
        Err(ConfigError::MissingField("VALUE"))
    ));
    assert!(matches!(
        config::parse_blocks("&OFFSET=zz&WIDTH=1&VALUE=01"),
        Err(ConfigError::BadNumber("OFFSET"))
    ));
    assert!(matches!(
        config::parse_blocks("&OFFSET=10000&WIDTH=1&VALUE=01"),
        Err(ConfigError::BadNumber("OFFSET"))
    ));
    assert!(matches!(
        config::parse_blocks("&ALTCFG=0"),
        Err(ConfigError::BadElement)
    ));
    assert!(config::parse_blocks("&").unwrap().is_empty());
}

#[test]
fn malformed_names_are_refused() {
    assert!(matches!(
        config::parse_names("&First=0"),
        Err(ConfigError::BadHex("VALUE"))
    ));
    assert!(matches!(
        config::parse_names("&First"),
        Err(ConfigError::BadElement)
    ));
    assert!(matches!(
        config::parse_names("&=00"),
        Err(ConfigError::BadElement)
    ));
}
