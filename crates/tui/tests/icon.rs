//! The app icon's resource (`crates/build-support/icon.rs`, linked by the
//! build scripts) is laid out as the linker and LoadImage read it.

#[path = "../../build-support/icon.rs"]
#[allow(dead_code)]
mod icon;

#[test]
fn the_res_holds_every_image_then_the_group_of_id_one() {
    let u16_at = |b: &[u8], at: usize| u16::from_le_bytes([b[at], b[at + 1]]);
    let u32_at = |b: &[u8], at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    let res = icon::res();
    // The empty opening entry.
    assert_eq!(u32_at(&res, 0), 0);
    assert_eq!(u32_at(&res, 4), 32);
    let mut at = 32;
    let mut kinds = Vec::new();
    while at < res.len() {
        let size = u32_at(&res, at) as usize;
        let (kind, name) = (u16_at(&res, at + 10), u16_at(&res, at + 14));
        kinds.push((kind, name));
        let data = &res[at + 32..at + 32 + size];
        if kind == 3 {
            // A DIB of twice the side, 32 bits a pixel.
            let side = u32_at(data, 4);
            assert_eq!(u32_at(data, 8), side * 2);
            assert_eq!(u16_at(data, 14), 32);
            assert!(icon::SIDES.contains(&side));
        } else {
            assert_eq!((kind, name), (14, 1), "the group, id 1");
            assert_eq!(u16_at(data, 4) as usize, icon::SIDES.len());
            assert_eq!(data.len(), 6 + 14 * icon::SIDES.len());
        }
        at += (32 + size).next_multiple_of(4);
    }
    assert_eq!(at, res.len());
    assert_eq!(kinds.len(), icon::SIDES.len() + 1);
}
