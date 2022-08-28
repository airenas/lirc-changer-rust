#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Event {
    pub id: String,
    pub repeat: u32,
    pub name: String,
    pub device: String,
}

impl Event {
    pub fn from_str(line: &str) -> Result<Self, String> {
        let strs = line.split_whitespace().collect::<Vec<_>>();
        if strs.len() != 4 {
            return Err(format!("'{}' is not of len 4", line));
        }
        //todo: parse as hexadecimal string
        match u32::from_str_radix(strs[1], 16) {
            Ok(n) => Ok(Event {
                id: strs[0].to_string(),
                repeat: n,
                name: strs[2].to_string(),
                device: strs[3].to_string(),
            }),
            Err(e) => Err(format!("can't parse '{}': {}", strs[1], e)),
        }
    }

    pub fn to_str(&self) -> String {
        format!(
            "{} {:x} {} {}",
            self.id, self.repeat, self.name, self.device
        )
    }

    pub fn to_hold(&self) -> Event {
        Event {
            id: self.id.clone(),
            repeat: 0,
            name: self.name.clone() + "_HOLD",
            device: self.device.clone(),
        }
    }

    pub fn to_new(&self) -> Event {
        Event {
            id: self.id.clone(),
            repeat: 0,
            name: self.name.clone(),
            device: self.device.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::event::Event;
    #[test]
    fn from_str() {
        assert_eq!(Event::from_str(""), Err(String::from("'' is not of len 4")));
        assert_eq!(
            Event::from_str("a 9 e"),
            Err(String::from("'a 9 e' is not of len 4"))
        );
        assert_eq!(
            Event::from_str("a 9 e d"),
            Ok(Event {
                id: String::from("a"),
                repeat: 9,
                name: String::from("e"),
                device: String::from("d")
            })
        );
    }

    #[test]
    fn from_str_hex() {
        assert_eq!(
            Event::from_str("a b e d"),
            Ok(Event {
                id: String::from("a"),
                repeat: 11,
                name: String::from("e"),
                device: String::from("d")
            })
        );
    }

    #[test]
    fn to_hold() {
        let e = Event::from_str("a b e d").unwrap();
        assert_eq!(
            e.to_hold(),
            Event {
                id: String::from("a"),
                repeat: 0,
                name: String::from("e_HOLD"),
                device: String::from("d")
            }
        );
    }

    #[test]
    fn to_str() {
        let e = Event {
            id: String::from("a"),
            repeat: 11,
            name: String::from("e"),
            device: String::from("d"),
        };
        assert_eq!(e.to_str(), "a b e d");
        let e = Event {
            id: String::from("a"),
            repeat: 16,
            name: String::from("e"),
            device: String::from("d"),
        };
        assert_eq!(e.to_str(), "a 10 e d");
    }

    #[test]
    fn to_new() {
        let e = Event {
            id: String::from("a"),
            repeat: 11,
            name: String::from("e"),
            device: String::from("d"),
        };
        assert_eq!(
            e.to_new(),
            Event {
                id: String::from("a"),
                repeat: 0,
                name: String::from("e"),
                device: String::from("d")
            }
        );
    }
}
