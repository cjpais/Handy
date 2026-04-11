use enigo::{Direction, Key};
use log::debug;
use serde::{Deserialize, Serialize};
use tungstenite::{Message, Utf8Bytes};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrowserEvent {
    ReadyForText,
    Text(String),
    KeyDown(String, String),
    KeyUp(String, String),
    MouseDown(u32),
    MouseUp(u32),
    MouseMove((i32, i32), (i32, i32)), // (relative, absolute)
    MouseScroll(i32, i32),
    Open,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrowserEventError {
    UnknownMessageType,
    ParseError,
}

impl TryFrom<Message> for BrowserEvent {
    type Error = BrowserEventError;

    fn try_from(message: Message) -> Result<Self, Self::Error> {
        match message {
            Message::Close(_) => {
                println!("Message::Close received");
                Ok(BrowserEvent::Close)
            }
            Message::Text(msg) => {
                println!("Browser received input");
                println!("msg: {msg:?}");

                // Attempt to deserialize the text message into a BrowserEvent
                if let Ok(event) = ron::from_str::<BrowserEvent>(&msg) {
                    Ok(event)
                } else {
                    println!("Parse error! Message: {msg}");
                    Err(BrowserEventError::ParseError)
                }
            }
            _ => {
                println!("Other Message received");
                Err(BrowserEventError::UnknownMessageType)
            }
        }
    }
}

impl PartialEq<(Key, Direction)> for BrowserEvent {
    fn eq(&self, (key, direction): &(Key, Direction)) -> bool {
        match self {
            BrowserEvent::KeyDown(name, debug_data) if *direction == Direction::Press => {
                let key_name = match key {
                    Key::Unicode(char) => format!("{char}"),
                    Key::Shift => "ShiftLeft".to_string(),
                    Key::LShift => "ShiftLeft".to_string(),
                    Key::RShift => "ShiftRight".to_string(),
                    Key::Control => "ControlLeft".to_string(),
                    Key::LControl => "ControlLeft".to_string(),
                    Key::RControl => "ControlRight".to_string(),
                    // TODO: Add the other keys that have a right and left variant here
                    _ => format!("{key:?}"),
                };
                if key_name == *name {
                    true
                } else {
                    debug!("key debug data: {debug_data}");
                    false
                }
            }

            BrowserEvent::KeyUp(name, debug_data) if *direction == Direction::Release => {
                let key_name = match key {
                    Key::Unicode(char) => format!("{char}"),
                    Key::Shift => "ShiftLeft".to_string(),
                    Key::LShift => "ShiftLeft".to_string(),
                    Key::RShift => "ShiftRight".to_string(),
                    Key::Control => "ControlLeft".to_string(),
                    Key::LControl => "ControlLeft".to_string(),
                    Key::RControl => "ControlRight".to_string(),
                    // TODO: Add the other keys that have a right and left variant here
                    _ => format!("{key:?}"),
                };
                if key_name == *name {
                    true
                } else {
                    debug!("{debug_data}");
                    false
                }
            }
            _ => false,
        }
    }
}

impl PartialEq<&str> for BrowserEvent {
    fn eq(&self, other: &&str) -> bool {
        if let BrowserEvent::Text(received_text) = self {
            other == received_text
        } else {
            false
        }
    }
}

#[test]
fn deserialize_browser_events() {
    let messages = vec![
        (
            Message::Text(Utf8Bytes::from("ReadyForText")),
            BrowserEvent::ReadyForText,
        ),
        (
            Message::Text(Utf8Bytes::from("Text(\"Testing\")")),
            BrowserEvent::Text("Testing".to_string()),
        ),
        (
            Message::Text(Utf8Bytes::from("Text(\"Hi how are you?❤️ äüß$3\")")),
            BrowserEvent::Text("Hi how are you?❤️ äüß$3".to_string()),
        ),
        (
            Message::Text(Utf8Bytes::from("KeyDown(\"F11\", \"\")")),
            BrowserEvent::KeyDown("F11".to_string(), "".to_string()),
        ),
        (
            Message::Text(Utf8Bytes::from("KeyUp(\"F11\", \"\")")),
            BrowserEvent::KeyUp("F11".to_string(), "".to_string()),
        ),
        (
            Message::Text(Utf8Bytes::from(
                "KeyDown(\"F1\", \"key: F1, which: 112, charCode: 0, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, repeat: false, isComposing: false, location: 0, bubbles: true, cancelable: true, defaultPrevented: false, composed: true\")",
            )),
            BrowserEvent::KeyDown("F1".to_string(),  "key: F1, which: 112, charCode: 0, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, repeat: false, isComposing: false, location: 0, bubbles: true, cancelable: true, defaultPrevented: false, composed: true".to_string()),
        ),
        (
            Message::Text(Utf8Bytes::from("MouseDown(0)")),
            BrowserEvent::MouseDown(0),
        ),
        (
            Message::Text(Utf8Bytes::from("MouseUp(0)")),
            BrowserEvent::MouseUp(0),
        ),
        (
            Message::Text(Utf8Bytes::from("MouseMove((-1806, -487), (200, 200))")),
            BrowserEvent::MouseMove((-1806, -487), (200, 200)),
        ),
        (
            Message::Text(Utf8Bytes::from("MouseScroll(3, -2)")),
            BrowserEvent::MouseScroll(3, -2),
        ),
    ];

    for (msg, event) in messages {
        let serialized = ron::to_string(&event).unwrap();
        println!("serialized = {serialized}");

        assert_eq!(BrowserEvent::try_from(msg).unwrap(), event);
    }
}
