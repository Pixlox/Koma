use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Text(&'a str),
    Number { raw: &'a str, trimmed: &'a str },
}

fn tokens(input: &str) -> Vec<Token<'_>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut in_number = input
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit());

    for (index, character) in input.char_indices() {
        let is_number = character.is_ascii_digit();
        if index > start && is_number != in_number {
            let raw = &input[start..index];
            result.push(if in_number {
                Token::Number {
                    raw,
                    trimmed: raw.trim_start_matches('0'),
                }
            } else {
                Token::Text(raw)
            });
            start = index;
            in_number = is_number;
        }
    }

    if start < input.len() {
        let raw = &input[start..];
        result.push(if in_number {
            Token::Number {
                raw,
                trimmed: raw.trim_start_matches('0'),
            }
        } else {
            Token::Text(raw)
        });
    }
    result
}

pub fn compare(left: &str, right: &str) -> Ordering {
    let left_lower = left.to_lowercase();
    let right_lower = right.to_lowercase();
    let left_tokens = tokens(&left_lower);
    let right_tokens = tokens(&right_lower);

    for (left_token, right_token) in left_tokens.iter().zip(right_tokens.iter()) {
        let ordering = match (left_token, right_token) {
            (Token::Text(left), Token::Text(right)) => left.cmp(right),
            (
                Token::Number {
                    raw: left_raw,
                    trimmed: left_trimmed,
                },
                Token::Number {
                    raw: right_raw,
                    trimmed: right_trimmed,
                },
            ) => {
                let left_value = if left_trimmed.is_empty() {
                    "0"
                } else {
                    left_trimmed
                };
                let right_value = if right_trimmed.is_empty() {
                    "0"
                } else {
                    right_trimmed
                };
                left_value
                    .len()
                    .cmp(&right_value.len())
                    .then_with(|| left_value.cmp(right_value))
                    .then_with(|| left_raw.len().cmp(&right_raw.len()))
            }
            (Token::Text(_), Token::Number { .. }) => Ordering::Less,
            (Token::Number { .. }, Token::Text(_)) => Ordering::Greater,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }

    left_tokens
        .len()
        .cmp(&right_tokens.len())
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::compare;

    #[test]
    fn sorts_page_numbers_naturally() {
        let mut pages = vec!["page 10.jpg", "page 2.jpg", "page 1.jpg"];
        pages.sort_by(|left, right| compare(left, right));
        assert_eq!(pages, vec!["page 1.jpg", "page 2.jpg", "page 10.jpg"]);
    }

    #[test]
    fn supports_large_numbers_without_overflowing() {
        assert!(compare("9", "100000000000000000000000000000000") < std::cmp::Ordering::Equal);
    }

    #[test]
    fn is_case_insensitive_with_stable_fallback() {
        assert!(compare("Page 2", "page 2") < std::cmp::Ordering::Equal);
    }
}
