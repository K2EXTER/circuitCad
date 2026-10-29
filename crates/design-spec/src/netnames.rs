//! What a net's name says about it: ground, a supply, a negative supply. (The board's checks and the bypass suggestions of the schematic
//! read the same names.)

pub fn is_ground_net(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    n.starts_with("GND") || n.ends_with("GND") || n == "VSS" || n == "0"
}

pub fn is_supply_net(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    // "V5", "V12", "V3V3" (a V and a digit), as well as "3V3" / "5V" and the VCC family
    let v_digit = n.strip_prefix('V').is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
    n.starts_with("VCC") || n.starts_with("VDD") || n.starts_with("VBUS") || n.starts_with('+') || n == "V+" || v_digit || (n.contains('V') && n.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// A negative supply: "VNEG", "VEE", "V-", "-5V", "-12V" ("VSS" is ground here).
pub fn is_negative_supply_net(net: &str) -> bool {
    let n = net.to_ascii_uppercase();
    n.starts_with("VNEG") || n.starts_with("VEE") || n == "V-" || n.starts_with('-') && n.contains('V')
}
