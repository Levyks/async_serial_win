use windows_sys::Win32::Devices::Communication::*;

/// Wire-format config passed from Dart. Field encodings are documented on the
/// Dart side (`lib/src/models.dart`) and must stay in sync with this layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FfiSerialConfig {
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: u8,
    pub flow_control: u8,
}

pub fn apply_to_dcb(dcb: &mut DCB, cfg: &FfiSerialConfig) {
    dcb.BaudRate = cfg.baud_rate;
    dcb.ByteSize = cfg.data_bits;

    dcb.StopBits = match cfg.stop_bits {
        1 => ONE5STOPBITS as u8,
        2 => TWOSTOPBITS as u8,
        _ => ONESTOPBIT as u8,
    };

    dcb.Parity = match cfg.parity {
        1 => ODDPARITY as u8,
        2 => EVENPARITY as u8,
        3 => MARKPARITY as u8,
        4 => SPACEPARITY as u8,
        _ => NOPARITY as u8,
    };

    // Clear then set the bitfield flags relevant to flow control / parity checking.
    let mut bits: u32 = dcb._bitfield;
    const F_BINARY: u32 = 1 << 0;
    const F_PARITY: u32 = 1 << 1;
    const F_OUT_X_CTS_FLOW: u32 = 1 << 2;
    const F_OUT_X_DSR_FLOW: u32 = 1 << 3;
    const F_DTR_CONTROL_MASK: u32 = 0b11 << 4;
    const F_DSR_SENSITIVITY: u32 = 1 << 6;
    const F_OUT_X: u32 = 1 << 8;
    const F_IN_X: u32 = 1 << 9;
    const F_RTS_CONTROL_MASK: u32 = 0b11 << 12;

    bits |= F_BINARY;
    bits &= !(F_OUT_X_CTS_FLOW
        | F_OUT_X_DSR_FLOW
        | F_DTR_CONTROL_MASK
        | F_DSR_SENSITIVITY
        | F_OUT_X
        | F_IN_X
        | F_RTS_CONTROL_MASK
        | F_PARITY);

    if cfg.parity != 0 {
        bits |= F_PARITY;
    }

    match cfg.flow_control {
        1 => {
            // RTS/CTS hardware flow control.
            bits |= F_OUT_X_CTS_FLOW;
            bits |= 0b10 << 12; // RTS_CONTROL_HANDSHAKE
            bits |= 0b01 << 4; // DTR_CONTROL_ENABLE
        }
        2 => {
            // XON/XOFF software flow control.
            bits |= F_OUT_X | F_IN_X;
            bits |= 0b01 << 4; // DTR_CONTROL_ENABLE
        }
        _ => {
            bits |= 0b01 << 4; // DTR_CONTROL_ENABLE
            bits |= 0b01 << 12; // RTS_CONTROL_ENABLE
        }
    }

    dcb._bitfield = bits;
    dcb.XonChar = 0x11;
    dcb.XoffChar = 0x13;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zeroed_dcb() -> DCB {
        unsafe { std::mem::zeroed() }
    }

    fn base_cfg() -> FfiSerialConfig {
        FfiSerialConfig {
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 0,
            parity: 0,
            flow_control: 0,
        }
    }

    const F_BINARY: u32 = 1 << 0;
    const F_PARITY: u32 = 1 << 1;
    const F_OUT_X_CTS_FLOW: u32 = 1 << 2;
    const F_OUT_X: u32 = 1 << 8;
    const F_IN_X: u32 = 1 << 9;
    const RTS_CONTROL_MASK: u32 = 0b11 << 12;
    const RTS_CONTROL_ENABLE: u32 = 0b01 << 12;
    const RTS_CONTROL_HANDSHAKE: u32 = 0b10 << 12;

    #[test]
    fn sets_baud_rate_and_data_bits() {
        let mut dcb = zeroed_dcb();
        let cfg = FfiSerialConfig { baud_rate: 115_200, data_bits: 7, ..base_cfg() };
        apply_to_dcb(&mut dcb, &cfg);
        assert_eq!(dcb.BaudRate, 115_200);
        assert_eq!(dcb.ByteSize, 7);
    }

    #[test]
    fn maps_stop_bits() {
        for (wire, expected) in [(0u8, ONESTOPBIT as u8), (1, ONE5STOPBITS as u8), (2, TWOSTOPBITS as u8)] {
            let mut dcb = zeroed_dcb();
            let cfg = FfiSerialConfig { stop_bits: wire, ..base_cfg() };
            apply_to_dcb(&mut dcb, &cfg);
            assert_eq!(dcb.StopBits, expected, "wire value {wire}");
        }
    }

    #[test]
    fn maps_parity_and_sets_parity_check_flag_only_when_nonzero() {
        let mut dcb = zeroed_dcb();
        apply_to_dcb(&mut dcb, &FfiSerialConfig { parity: 0, ..base_cfg() });
        assert_eq!(dcb.Parity, NOPARITY as u8);
        assert_eq!(dcb._bitfield & F_PARITY, 0, "parity checking should be off for None");

        for (wire, expected) in [(1u8, ODDPARITY as u8), (2, EVENPARITY as u8), (3, MARKPARITY as u8), (4, SPACEPARITY as u8)] {
            let mut dcb = zeroed_dcb();
            let cfg = FfiSerialConfig { parity: wire, ..base_cfg() };
            apply_to_dcb(&mut dcb, &cfg);
            assert_eq!(dcb.Parity, expected, "wire value {wire}");
            assert_ne!(dcb._bitfield & F_PARITY, 0, "parity checking should be on for wire value {wire}");
        }
    }

    #[test]
    fn flow_control_none_enables_rts_without_cts_flow() {
        let mut dcb = zeroed_dcb();
        apply_to_dcb(&mut dcb, &FfiSerialConfig { flow_control: 0, ..base_cfg() });
        assert_eq!(dcb._bitfield & RTS_CONTROL_MASK, RTS_CONTROL_ENABLE);
        assert_eq!(dcb._bitfield & F_OUT_X_CTS_FLOW, 0);
        assert_eq!(dcb._bitfield & (F_OUT_X | F_IN_X), 0);
    }

    #[test]
    fn flow_control_rts_cts_sets_handshake_and_cts_flow() {
        let mut dcb = zeroed_dcb();
        apply_to_dcb(&mut dcb, &FfiSerialConfig { flow_control: 1, ..base_cfg() });
        assert_eq!(dcb._bitfield & RTS_CONTROL_MASK, RTS_CONTROL_HANDSHAKE);
        assert_ne!(dcb._bitfield & F_OUT_X_CTS_FLOW, 0);
    }

    #[test]
    fn flow_control_xon_xoff_sets_software_flow_bits_and_chars() {
        let mut dcb = zeroed_dcb();
        apply_to_dcb(&mut dcb, &FfiSerialConfig { flow_control: 2, ..base_cfg() });
        assert_ne!(dcb._bitfield & F_OUT_X, 0);
        assert_ne!(dcb._bitfield & F_IN_X, 0);
        assert_eq!(dcb.XonChar, 0x11);
        assert_eq!(dcb.XoffChar, 0x13);
    }

    #[test]
    fn always_sets_binary_mode() {
        let mut dcb = zeroed_dcb();
        apply_to_dcb(&mut dcb, &base_cfg());
        assert_ne!(dcb._bitfield & F_BINARY, 0);
    }

    #[test]
    fn reapplying_a_different_flow_control_clears_the_previous_ones() {
        let mut dcb = zeroed_dcb();
        apply_to_dcb(&mut dcb, &FfiSerialConfig { flow_control: 1, ..base_cfg() });
        assert_ne!(dcb._bitfield & F_OUT_X_CTS_FLOW, 0);

        apply_to_dcb(&mut dcb, &FfiSerialConfig { flow_control: 2, ..base_cfg() });
        assert_eq!(dcb._bitfield & F_OUT_X_CTS_FLOW, 0, "stale CTS flow bit must be cleared");
        assert_ne!(dcb._bitfield & F_OUT_X, 0);
    }
}
