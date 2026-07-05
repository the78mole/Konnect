//! Protobuf message builders for KiCAD 10 IPC API.
//!
//! These helpers construct the protobuf messages needed to create, update, and
//! delete PCB items via the IPC API.

use crate::gen::kiapi;

/// Converts millimeters to KiCAD nanometers.
pub fn mm_to_nm(mm: f64) -> i64 {
    (mm * 1_000_000.0) as i64
}

/// Converts KiCAD nanometers to millimeters.
pub fn nm_to_mm(nm: i64) -> f64 {
    nm as f64 / 1_000_000.0
}

/// Build a Vector2 in nanometers from mm coordinates.
pub fn vec2(x_mm: f64, y_mm: f64) -> kiapi::common::types::Vector2 {
    kiapi::common::types::Vector2 {
        x_nm: mm_to_nm(x_mm),
        y_nm: mm_to_nm(y_mm),
    }
}

/// Build a Distance in nanometers from mm.
pub fn distance(mm: f64) -> kiapi::common::types::Distance {
    kiapi::common::types::Distance {
        value_nm: mm_to_nm(mm),
    }
}

/// Build a Net message.
pub fn net(name: &str, code: i32) -> kiapi::board::types::Net {
    kiapi::board::types::Net {
        code: Some(kiapi::board::types::NetCode { value: code }),
        name: name.to_string(),
    }
}

/// Map a layer name string to the BoardLayer enum value.
pub fn layer_from_name(name: &str) -> kiapi::board::types::BoardLayer {
    match name {
        "F.Cu" => kiapi::board::types::BoardLayer::BlFCu,
        "B.Cu" => kiapi::board::types::BoardLayer::BlBCu,
        "In1.Cu" => kiapi::board::types::BoardLayer::BlIn1Cu,
        "In2.Cu" => kiapi::board::types::BoardLayer::BlIn2Cu,
        "F.SilkS" | "F.Silkscreen" => kiapi::board::types::BoardLayer::BlFSilkS,
        "B.SilkS" | "B.Silkscreen" => kiapi::board::types::BoardLayer::BlBSilkS,
        "F.Mask" => kiapi::board::types::BoardLayer::BlFMask,
        "B.Mask" => kiapi::board::types::BoardLayer::BlBMask,
        "F.Paste" => kiapi::board::types::BoardLayer::BlFPaste,
        "B.Paste" => kiapi::board::types::BoardLayer::BlBPaste,
        "F.CrtYd" | "F.Courtyard" => kiapi::board::types::BoardLayer::BlFCrtYd,
        "B.CrtYd" | "B.Courtyard" => kiapi::board::types::BoardLayer::BlBCrtYd,
        "F.Fab" => kiapi::board::types::BoardLayer::BlFFab,
        "B.Fab" => kiapi::board::types::BoardLayer::BlBFab,
        "Edge.Cuts" => kiapi::board::types::BoardLayer::BlEdgeCuts,
        _ => kiapi::board::types::BoardLayer::BlUndefined,
    }
}

/// Build a Track protobuf message.
#[allow(clippy::too_many_arguments)]
pub fn build_track(
    net_name: &str,
    net_code: i32,
    layer: &str,
    width_mm: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
) -> kiapi::board::types::Track {
    kiapi::board::types::Track {
        id: None, // KiCAD assigns the ID
        start: Some(vec2(x1, y1)),
        end: Some(vec2(x2, y2)),
        width: Some(distance(width_mm)),
        locked: kiapi::common::types::LockedState::LsUnlocked as i32,
        layer: layer_from_name(layer) as i32,
        net: Some(net(net_name, net_code)),
    }
}

/// Build S-expression for a via (used with ParseAndCreateItemsFromString).
/// Complex protobuf PadStack construction is avoided this way.
pub fn via_sexp(
    net_name: &str,
    net_code: i32,
    x: f64,
    y: f64,
    drill_mm: f64,
    size_mm: f64,
) -> String {
    format!(
        r#"(via (at {} {}) (size {}) (drill {}) (layers "F.Cu" "B.Cu") (net {} "{}"))"#,
        x, y, size_mm, drill_mm, net_code, net_name
    )
}

/// Pack a protobuf message into a prost_types::Any.
pub fn pack_any<M: prost::Message>(msg: &M, type_name: &str) -> prost_types::Any {
    let mut buf = Vec::new();
    msg.encode(&mut buf).expect("protobuf encode failed");
    prost_types::Any {
        type_url: format!("type.googleapis.com/{}", type_name),
        value: buf,
    }
}
