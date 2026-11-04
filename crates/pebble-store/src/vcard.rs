use std::collections::HashSet;

use pebble_core::{
    Contact, ContactEmailInput, ContactEmailLabel, ContactInput, PebbleError, Result,
    VcardImportResult,
};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    contacts::{load_contact_with_conn, save_contact_with_conn, MAX_CONTACT_DISPLAY_NAME_CHARS},
    Store,
};

const MAX_VCARD_BYTES: usize = 5 * 1024 * 1024;
const MAX_VCARD_CONTACTS: usize = 10_000;
const MAX_IMPORT_ERRORS: usize = 20;

#[derive(Debug)]
struct ParsedEmail {
    address: String,
    label: ContactEmailLabel,
    preferred: bool,
}

fn unescape_value(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => output.push('\n'),
            Some('\\') => output.push('\\'),
            Some(',') => output.push(','),
            Some(';') => output.push(';'),
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

fn escape_value(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

fn split_escaped(value: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push('\\');
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == separator {
            parts.push(current);
            current = String::new();
        } else {
            current.push(ch);
        }
    }
    if escaped {
        current.push('\\');
    }
    parts.push(current);
    parts
}

fn split_once_unquoted(value: &str, separator: char) -> Option<(&str, &str)> {
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if ch == separator && !quoted {
            let separator_end = index + ch.len_utf8();
            return Some((&value[..index], &value[separator_end..]));
        }
    }
    None
}

fn split_unquoted(value: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if ch == separator && !quoted {
            parts.push(&value[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&value[start..]);
    parts
}

fn name_from_n(value: &str) -> String {
    let fields = split_escaped(value, ';');
    let decoded = fields
        .iter()
        .map(|field| unescape_value(field).trim().to_string())
        .collect::<Vec<_>>();
    [
        decoded.get(3),
        decoded.get(1),
        decoded.get(2),
        decoded.first(),
        decoded.get(4),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join(" ")
}

fn parse_email_header(header: &str) -> (ContactEmailLabel, bool) {
    let tokens = split_unquoted(header, ';')
        .into_iter()
        .skip(1)
        .flat_map(|part| {
            let (name, value) = split_once_unquoted(part, '=')
                .map(|(name, value)| (Some(name.trim()), value.trim()))
                .unwrap_or((None, part.trim()));
            if name.is_some_and(|name| !name.eq_ignore_ascii_case("TYPE")) {
                return Vec::new();
            }
            value
                .trim_matches('"')
                .split(',')
                .map(|token| token.trim().trim_matches('"').to_ascii_uppercase())
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>()
        })
        .collect::<HashSet<_>>();
    let label = if tokens.contains("WORK") {
        ContactEmailLabel::Work
    } else if tokens.contains("HOME") || tokens.contains("PERSONAL") {
        ContactEmailLabel::Personal
    } else {
        ContactEmailLabel::Other
    };
    (label, tokens.contains("PREF"))
}

fn parse_card(lines: &[String]) -> std::result::Result<ContactInput, String> {
    let mut version = None;
    let mut display_name = String::new();
    let mut structured_name = String::new();
    let mut notes = Vec::new();
    let mut parsed_emails = Vec::new();

    for line in lines {
        let Some((header, raw_value)) = split_once_unquoted(line, ':') else {
            continue;
        };
        let property = split_unquoted(header, ';')
            .into_iter()
            .next()
            .unwrap_or_default()
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        match property.as_str() {
            "VERSION" => version = Some(raw_value.trim().to_string()),
            "FN" => display_name = unescape_value(raw_value).trim().to_string(),
            "N" => structured_name = name_from_n(raw_value),
            "NOTE" => notes.push(unescape_value(raw_value)),
            "EMAIL" => {
                let address = unescape_value(raw_value).trim().to_string();
                if !address.is_empty() {
                    let (label, preferred) = parse_email_header(header);
                    parsed_emails.push(ParsedEmail {
                        address,
                        label,
                        preferred,
                    });
                }
            }
            _ => {}
        }
    }

    if version.as_deref() != Some("3.0") {
        return Err("Only vCard 3.0 is supported".to_string());
    }
    if parsed_emails.is_empty() {
        return Err("Contact has no email address".to_string());
    }

    let mut seen = HashSet::new();
    parsed_emails.retain(|email| seen.insert(email.address.to_lowercase()));
    let preferred_index = parsed_emails
        .iter()
        .position(|email| email.preferred)
        .unwrap_or(0);
    let emails = parsed_emails
        .into_iter()
        .enumerate()
        .map(|(index, email)| ContactEmailInput {
            id: None,
            address: email.address,
            label: email.label,
            is_primary: index == preferred_index,
        })
        .collect();

    let display_name = if display_name.is_empty() {
        structured_name
    } else {
        display_name
    };
    if display_name.chars().count() > MAX_CONTACT_DISPLAY_NAME_CHARS {
        return Err(format!(
            "Contact display name must not exceed {MAX_CONTACT_DISPLAY_NAME_CHARS} characters"
        ));
    }

    Ok(ContactInput {
        id: None,
        display_name,
        notes: notes.join("\n").trim().to_string(),
        is_favorite: false,
        emails,
    })
}

fn parse_vcards(data: &str) -> Result<Vec<std::result::Result<ContactInput, String>>> {
    if data.len() > MAX_VCARD_BYTES {
        return Err(PebbleError::Validation(
            "vCard import must not exceed 5 MiB".to_string(),
        ));
    }

    let normalized = data.replace("\r\n", "\n").replace('\r', "\n");
    let mut unfolded: Vec<String> = Vec::new();
    for raw_line in normalized.lines() {
        if raw_line.starts_with([' ', '\t']) {
            if let Some(previous) = unfolded.last_mut() {
                previous.push_str(&raw_line[1..]);
            }
        } else {
            unfolded.push(raw_line.to_string());
        }
    }

    let card_count = unfolded
        .iter()
        .filter(|line| line.eq_ignore_ascii_case("BEGIN:VCARD"))
        .count();
    if card_count > MAX_VCARD_CONTACTS {
        return Err(PebbleError::Validation(
            "vCard import must not contain more than 10,000 contacts".to_string(),
        ));
    }

    struct CardState {
        lines: Vec<String>,
        nested_depth: usize,
        error: Option<String>,
    }

    let mut results = Vec::with_capacity(card_count);
    let mut current: Option<CardState> = None;
    for line in unfolded {
        if line.eq_ignore_ascii_case("BEGIN:VCARD") {
            if let Some(card) = current.as_mut() {
                card.nested_depth += 1;
                card.error
                    .get_or_insert_with(|| "Nested BEGIN:VCARD".to_string());
            } else {
                current = Some(CardState {
                    lines: Vec::new(),
                    nested_depth: 0,
                    error: None,
                });
            }
        } else if line.eq_ignore_ascii_case("END:VCARD") {
            if let Some(card) = current.as_mut() {
                if card.nested_depth > 0 {
                    card.nested_depth -= 1;
                    continue;
                }
            }
            if let Some(card) = current.take() {
                results.push(match card.error {
                    Some(error) => Err(error),
                    None => parse_card(&card.lines),
                });
            }
        } else if let Some(card) = current.as_mut() {
            if card.nested_depth == 0 {
                card.lines.push(line);
            }
        }
    }
    if let Some(card) = current {
        results.push(Err(card
            .error
            .unwrap_or_else(|| "Missing END:VCARD".to_string())));
    }
    Ok(results)
}

fn owner_ids_for_input(conn: &Connection, input: &ContactInput) -> Result<HashSet<String>> {
    let mut owners = HashSet::new();
    for email in &input.emails {
        let owner = conn
            .query_row(
                "SELECT contact_id FROM contact_emails WHERE normalized_address = ?1 COLLATE NOCASE",
                params![email.address.trim().to_lowercase()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(owner) = owner {
            owners.insert(owner);
        }
    }
    Ok(owners)
}

fn merge_input(existing: &Contact, imported: ContactInput) -> Option<ContactInput> {
    let mut emails = existing
        .emails
        .iter()
        .map(|email| ContactEmailInput {
            id: Some(email.id.clone()),
            address: email.address.clone(),
            label: email.label.clone(),
            is_primary: email.is_primary,
        })
        .collect::<Vec<_>>();
    let mut known = emails
        .iter()
        .map(|email| email.address.to_lowercase())
        .collect::<HashSet<_>>();
    for email in imported.emails {
        if known.insert(email.address.to_lowercase()) {
            emails.push(ContactEmailInput {
                is_primary: false,
                ..email
            });
        }
    }
    let display_name = if existing.display_name.trim().is_empty() {
        imported.display_name
    } else {
        existing.display_name.clone()
    };
    let notes = if existing.notes.trim().is_empty() {
        imported.notes
    } else {
        existing.notes.clone()
    };
    if emails.len() == existing.emails.len()
        && display_name == existing.display_name
        && notes == existing.notes
    {
        return None;
    }
    Some(ContactInput {
        id: Some(existing.id.clone()),
        display_name,
        notes,
        is_favorite: existing.is_favorite,
        emails,
    })
}

fn push_import_error(result: &mut VcardImportResult, index: usize, message: impl Into<String>) {
    result.invalid += 1;
    if result.errors.len() < MAX_IMPORT_ERRORS {
        result
            .errors
            .push(format!("Card {}: {}", index + 1, message.into()));
    }
}

fn import_with_conn(
    conn: &Connection,
    cards: Vec<std::result::Result<ContactInput, String>>,
) -> Result<VcardImportResult> {
    let mut result = VcardImportResult::default();
    for (index, card) in cards.into_iter().enumerate() {
        let input = match card {
            Ok(input) => input,
            Err(message) => {
                push_import_error(&mut result, index, message);
                continue;
            }
        };
        let owners = owner_ids_for_input(conn, &input)?;
        let (input, merged) = match owners.len() {
            0 => (input, false),
            1 => {
                let owner = owners.into_iter().next().unwrap_or_default();
                let existing = load_contact_with_conn(conn, &owner)?.ok_or_else(|| {
                    PebbleError::Internal("Existing contact could not be loaded".to_string())
                })?;
                let Some(merged_input) = merge_input(&existing, input) else {
                    result.skipped += 1;
                    continue;
                };
                (merged_input, true)
            }
            _ => {
                push_import_error(
                    &mut result,
                    index,
                    "Email addresses belong to multiple existing contacts",
                );
                continue;
            }
        };
        match save_contact_with_conn(conn, &input) {
            Ok(_) if merged => result.merged += 1,
            Ok(_) => result.created += 1,
            Err(PebbleError::Validation(message)) => push_import_error(&mut result, index, message),
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}

fn fold_line(line: &str) -> String {
    if line.len() <= 75 {
        return format!("{line}\r\n");
    }
    let mut output = String::new();
    let mut remaining = line;
    let mut first = true;
    while !remaining.is_empty() {
        let limit = if first { 75 } else { 74 };
        let mut end = remaining.len().min(limit);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        if !first {
            output.push(' ');
        }
        output.push_str(&remaining[..end]);
        output.push_str("\r\n");
        remaining = &remaining[end..];
        first = false;
    }
    output
}

fn serialize_contact(contact: &Contact) -> String {
    let mut output = String::new();
    for line in [
        "BEGIN:VCARD".to_string(),
        "VERSION:3.0".to_string(),
        format!("FN:{}", escape_value(&contact.display_name)),
        "N:;;;;".to_string(),
    ] {
        output.push_str(&fold_line(&line));
    }
    for email in &contact.emails {
        let label = match email.label {
            ContactEmailLabel::Work => "WORK",
            ContactEmailLabel::Personal => "HOME",
            ContactEmailLabel::Other => "INTERNET",
        };
        let preferred = if email.is_primary { ",PREF" } else { "" };
        output.push_str(&fold_line(&format!(
            "EMAIL;TYPE={label}{preferred}:{}",
            escape_value(&email.address)
        )));
    }
    if !contact.notes.is_empty() {
        output.push_str(&fold_line(&format!(
            "NOTE:{}",
            escape_value(&contact.notes)
        )));
    }
    output.push_str("END:VCARD\r\n");
    output
}

impl Store {
    pub fn import_contacts_vcard(&self, data: &str) -> Result<VcardImportResult> {
        let cards = parse_vcards(data)?;
        self.with_write(|conn| {
            let tx = conn.unchecked_transaction()?;
            let result = import_with_conn(&tx, cards)?;
            tx.commit()?;
            Ok(result)
        })
    }

    pub fn export_contacts_vcard(&self) -> Result<String> {
        self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id FROM contacts ORDER BY display_name COLLATE NOCASE ASC, created_at ASC",
            )?;
            let ids = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut output = String::new();
            for id in ids {
                if let Some(contact) = load_contact_with_conn(conn, &id)? {
                    output.push_str(&serialize_contact(&contact));
                }
            }
            Ok(output)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_card;
    use pebble_core::{ContactEmailInput, ContactEmailLabel, ContactInput, PebbleError};

    use crate::Store;

    const COMPLEX_CARD: &str = concat!(
        "BEGIN:VCARD\r\n",
        "VERSION:3.0\r\n",
        "FN:张三\r\n",
        "EMAIL;TYPE=WORK,PREF:Zhang.San@example.com\r\n",
        "EMAIL;TYPE=HOME:zhang@example.net\r\n",
        "NOTE:第一行\\n第二行\\,有逗号\\;有分号并且很\r\n",
        " 长\r\n",
        "END:VCARD\r\n",
    );

    fn contact_input(address: &str) -> ContactInput {
        ContactInput {
            id: None,
            display_name: "Existing".to_string(),
            notes: "existing note".to_string(),
            is_favorite: true,
            emails: vec![ContactEmailInput {
                id: None,
                address: address.to_string(),
                label: ContactEmailLabel::Other,
                is_primary: true,
            }],
        }
    }

    #[test]
    fn export_import_preserves_whitespace_at_fold_boundaries() {
        for whitespace in [" ", "\t", "  "] {
            let source = Store::open_in_memory().unwrap();
            let mut input = contact_input("fold@example.com");
            input.display_name = format!("{}{whitespace}B", "A".repeat(72));
            input.notes = format!("{}{whitespace}B", "N".repeat(70));
            source.save_contact(&input).unwrap();
            let destination = Store::open_in_memory().unwrap();
            destination
                .import_contacts_vcard(&source.export_contacts_vcard().unwrap())
                .unwrap();
            let restored = destination
                .list_contacts(None, false, 10, 0)
                .unwrap()
                .remove(0);
            assert_eq!(restored.display_name, input.display_name);
            assert_eq!(restored.notes, input.notes);
        }
    }

    #[test]
    fn export_normalizes_carriage_returns_without_injecting_properties() {
        for line_break in ["\r", "\r\n", "\n"] {
            let source = Store::open_in_memory().unwrap();
            let mut input = contact_input("original@example.com");
            input.notes = format!("Hello{line_break}EMAIL:injected@example.com");
            source.save_contact(&input).unwrap();
            let destination = Store::open_in_memory().unwrap();
            destination
                .import_contacts_vcard(&source.export_contacts_vcard().unwrap())
                .unwrap();
            let restored = destination
                .list_contacts(None, false, 10, 0)
                .unwrap()
                .remove(0);
            assert_eq!(
                restored.emails.len(),
                1,
                "Text must not become another vCard property"
            );
            assert_eq!(restored.notes, "Hello\nEMAIL:injected@example.com");
        }
    }

    #[test]
    fn imports_utf8_multiple_emails_folded_lines_and_escapes() {
        let store = Store::open_in_memory().unwrap();

        let result = store.import_contacts_vcard(COMPLEX_CARD).unwrap();

        assert_eq!(result.created, 1);
        assert_eq!(result.merged, 0);
        assert_eq!(result.invalid, 0);
        let contacts = store.list_contacts(None, false, 20, 0).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].display_name, "张三");
        assert_eq!(contacts[0].emails.len(), 2);
        assert_eq!(contacts[0].emails[0].label, ContactEmailLabel::Work);
        assert!(contacts[0].emails[0].is_primary);
        assert_eq!(contacts[0].emails[1].label, ContactEmailLabel::Personal);
        assert_eq!(contacts[0].notes, "第一行\n第二行,有逗号;有分号并且很长");
    }

    #[test]
    fn imports_name_from_n_when_fn_is_missing() {
        let store = Store::open_in_memory().unwrap();
        let data =
            "BEGIN:VCARD\nVERSION:3.0\nN:Lovelace;Ada;;;\nEMAIL:ada@example.com\nEND:VCARD\n";

        store.import_contacts_vcard(data).unwrap();

        let contacts = store.list_contacts(None, false, 20, 0).unwrap();
        assert_eq!(contacts[0].display_name, "Ada Lovelace");
    }

    #[test]
    fn imports_quoted_type_parameter_tokens() {
        let store = Store::open_in_memory().unwrap();
        let data = "BEGIN:VCARD\nVERSION:3.0\nFN:Alice\nEMAIL;TYPE=\"WORK,PREF\":alice@example.com\nEMAIL;TYPE=\"HOME\":alice@home.example\nEND:VCARD\n";

        store.import_contacts_vcard(data).unwrap();

        let contact = &store.list_contacts(None, false, 20, 0).unwrap()[0];
        assert_eq!(contact.emails[0].label, ContactEmailLabel::Work);
        assert!(contact.emails[0].is_primary);
        assert_eq!(contact.emails[1].label, ContactEmailLabel::Personal);
    }

    #[test]
    fn rejects_cards_without_version_three() {
        let store = Store::open_in_memory().unwrap();
        let data = "BEGIN:VCARD\nFN:Alice\nEMAIL:alice@example.com\nEND:VCARD\n";

        let result = store.import_contacts_vcard(data).unwrap();

        assert_eq!(result.created, 0);
        assert_eq!(result.invalid, 1);
        assert!(result.errors[0].contains("vCard 3.0"));
    }

    #[test]
    fn nested_cards_are_rejected_without_importing_the_inner_fragment() {
        let store = Store::open_in_memory().unwrap();
        let data = "BEGIN:VCARD\nVERSION:3.0\nFN:Outer\n\
                    BEGIN:VCARD\nVERSION:3.0\nFN:Inner\nEMAIL:inner@example.com\nEND:VCARD\n\
                    EMAIL:outer@example.com\nEND:VCARD\n";

        let result = store.import_contacts_vcard(data).unwrap();

        assert_eq!(result.created, 0);
        assert_eq!(result.invalid, 1);
        assert!(result.errors[0].contains("Nested BEGIN:VCARD"));
        assert!(store.list_contacts(None, false, 20, 0).unwrap().is_empty());
    }

    #[test]
    fn quoted_custom_parameters_may_contain_property_delimiters() {
        let store = Store::open_in_memory().unwrap();
        let data = "BEGIN:VCARD\nVERSION:3.0\nFN:Alice\n\
                    EMAIL;X-FOO=\"a:b=c;d\";TYPE=WORK:alice@example.com\nEND:VCARD\n";

        let result = store.import_contacts_vcard(data).unwrap();

        assert_eq!(result.created, 1);
        assert_eq!(result.invalid, 0);
        let contact = store.list_contacts(None, false, 20, 0).unwrap().remove(0);
        assert_eq!(contact.emails[0].address, "alice@example.com");
        assert_eq!(contact.emails[0].label, ContactEmailLabel::Work);
    }

    #[test]
    fn parse_card_rejects_display_names_over_limit() {
        let lines = vec![
            "VERSION:3.0".to_string(),
            format!("FN:{}", "a".repeat(513)),
            "EMAIL:alice@example.com".to_string(),
        ];

        let error = parse_card(&lines).unwrap_err();

        assert!(error.contains("512"));
    }

    #[test]
    fn merges_duplicate_addresses_into_an_existing_contact() {
        let store = Store::open_in_memory().unwrap();
        store
            .save_contact(&contact_input("Alice@example.com"))
            .unwrap();
        let data = "BEGIN:VCARD\nVERSION:3.0\nFN:Alice Imported\nEMAIL;TYPE=PREF:alice@EXAMPLE.com\nEMAIL;TYPE=WORK:alice.work@example.com\nEND:VCARD\n";

        let result = store.import_contacts_vcard(data).unwrap();

        assert_eq!(result.created, 0);
        assert_eq!(result.merged, 1);
        let contacts = store.list_contacts(None, false, 20, 0).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].display_name, "Existing");
        assert!(contacts[0].is_favorite);
        assert_eq!(contacts[0].emails.len(), 2);
    }

    #[test]
    fn reports_partial_invalid_records_without_losing_valid_contacts() {
        let store = Store::open_in_memory().unwrap();
        let data = "BEGIN:VCARD\nVERSION:3.0\nFN:No Email\nEND:VCARD\n\
                    BEGIN:VCARD\nVERSION:3.0\nFN:Valid\nEMAIL:valid@example.com\nEND:VCARD\n";

        let result = store.import_contacts_vcard(data).unwrap();

        assert_eq!(result.created, 1);
        assert_eq!(result.invalid, 1);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(store.list_contacts(None, false, 20, 0).unwrap().len(), 1);
    }

    #[test]
    fn rejects_files_larger_than_five_mib() {
        let store = Store::open_in_memory().unwrap();
        let oversized = "X".repeat(5 * 1024 * 1024 + 1);

        assert!(matches!(
            store.import_contacts_vcard(&oversized),
            Err(PebbleError::Validation(message)) if message.contains("5 MiB")
        ));
    }

    #[test]
    fn rejects_more_than_ten_thousand_cards() {
        let store = Store::open_in_memory().unwrap();
        let data = (0..10_001)
            .map(|index| {
                format!("BEGIN:VCARD\nVERSION:3.0\nEMAIL:user{index}@example.com\nEND:VCARD\n")
            })
            .collect::<String>();

        assert!(matches!(
            store.import_contacts_vcard(&data),
            Err(PebbleError::Validation(message)) if message.contains("10,000")
        ));
    }

    #[test]
    fn exported_contacts_round_trip_with_names_notes_and_labels() {
        let source = Store::open_in_memory().unwrap();
        source
            .save_contact(&ContactInput {
                id: None,
                display_name: "Zoë, Example".to_string(),
                notes: "Line one\nLine two; detail".to_string(),
                is_favorite: false,
                emails: vec![
                    ContactEmailInput {
                        id: None,
                        address: "zoe@example.com".to_string(),
                        label: ContactEmailLabel::Personal,
                        is_primary: true,
                    },
                    ContactEmailInput {
                        id: None,
                        address: "work@example.com".to_string(),
                        label: ContactEmailLabel::Work,
                        is_primary: false,
                    },
                ],
            })
            .unwrap();

        let exported = source.export_contacts_vcard().unwrap();
        assert!(exported.contains("VERSION:3.0\r\n"));
        assert!(exported.contains("FN:Zoë\\, Example\r\n"));

        let restored = Store::open_in_memory().unwrap();
        let result = restored.import_contacts_vcard(&exported).unwrap();
        assert_eq!(result.created, 1);
        let contact = &restored.list_contacts(None, false, 20, 0).unwrap()[0];
        assert_eq!(contact.display_name, "Zoë, Example");
        assert_eq!(contact.notes, "Line one\nLine two; detail");
        assert_eq!(contact.emails.len(), 2);
        assert_eq!(contact.emails[0].label, ContactEmailLabel::Personal);
        assert!(contact.emails[0].is_primary);
    }
}
