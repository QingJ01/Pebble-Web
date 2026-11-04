//! Sender validation and RFC-compliant encoding shared by SMTP and Gmail.
use lettre::message::header::Headers;
use lettre::message::{header, Mailbox, Mailboxes};
use pebble_core::{EmailAddress, PebbleError, Result};

pub fn sender_mailbox(from: &EmailAddress) -> Result<Mailbox> {
    if from.address.chars().any(char::is_control)
        || from
            .name
            .as_deref()
            .is_some_and(|name| name.chars().any(char::is_control))
    {
        return Err(PebbleError::Validation(
            "Sender identity contains control characters".into(),
        ));
    }
    let address = from
        .address
        .trim()
        .parse()
        .map_err(|_| PebbleError::Validation("Sender email address is invalid".into()))?;
    let name = from
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    Ok(Mailbox::new(name, address))
}

pub fn from_header(from: &EmailAddress) -> Result<String> {
    let mut headers = Headers::new();
    let mailboxes: Mailboxes = std::iter::once(sender_mailbox(from)?).collect();
    headers.set(header::From::from(mailboxes));
    Ok(headers.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::MessageParser;

    #[test]
    fn sender_header_round_trips_unicode_punctuation_and_empty_names() {
        let long_name = "张三团队 ".repeat(40);
        for name in [
            "张三",
            "Doe, Jane",
            "Jane \"JJ\" Doe",
            "",
            "   ",
            &long_name,
        ] {
            let from = EmailAddress {
                name: Some(name.into()),
                address: "sender@example.com".into(),
            };
            let raw = format!(
                "{}To: to@example.com\r\n\r\nHello",
                from_header(&from).unwrap()
            );
            let parsed = MessageParser::default().parse(raw.as_bytes()).unwrap();
            let parsed_from = parsed.from().unwrap().first().unwrap();
            assert_eq!(parsed_from.address.as_deref(), Some("sender@example.com"));
            let expected_name = (!name.trim().is_empty()).then_some(name.trim());
            assert_eq!(parsed_from.name.as_deref(), expected_name);
            assert_eq!(
                raw.lines().filter(|line| line.starts_with("From:")).count(),
                1
            );
        }
    }

    #[test]
    fn sender_rejects_header_injection_and_embedded_mailboxes() {
        for name in ["Bad\r\nBcc: other@example.com", "Bad\0name", "Bad\nname"] {
            assert!(sender_mailbox(&EmailAddress {
                name: Some(name.into()),
                address: "sender@example.com".into()
            })
            .is_err());
        }
        for address in [
            "",
            "Name <sender@example.com>",
            "sender@example.com\r\nBcc: other@example.com",
        ] {
            assert!(sender_mailbox(&EmailAddress {
                name: None,
                address: address.into()
            })
            .is_err());
        }
    }
}
