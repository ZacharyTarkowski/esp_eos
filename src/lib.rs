#![no_std]

#[derive(Debug)]
pub enum MsgType {
    Unknown,
    WifiSSID,
    WifiPass,
}

impl From<u8> for MsgType {
    fn from(val: u8) -> Self {
        match val {
            1 => MsgType::WifiSSID,
            2 => MsgType::WifiPass,
            _ => MsgType::Unknown,
        }
    }
}

impl From<MsgType> for u8 {
    fn from(msg: MsgType) -> Self {
        match msg {
            MsgType::WifiSSID => 1,
            MsgType::WifiPass => 2,
            _ => 0,
        }
    }
}
