//! Reads mysqldump output statement by statement and parses its `CREATE
//! TABLE` and `INSERT` statements, without a database.
//!
//! Assumes MySQL's default escaping (backslash escapes in strings), which is
//! what mysqldump writes unless `NO_BACKSLASH_ESCAPES` is set.

use std::{borrow::Cow, io, io::BufRead, ops::Range};

/// Reads a dump statement by statement: `INSERT`, `REPLACE` and `CREATE
/// TABLE` statements whole (also when a string in them spans lines),
/// everything else line by line.
pub struct Statements<R> {
	reader: R,
}

impl<R: BufRead> Statements<R> {
	pub fn new(reader: R) -> Self {
		Self { reader }
	}

	/// Reads the next statement into `buf`, with its line breaks. Returns
	/// false at the end of the dump.
	pub fn next(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
		buf.clear();
		if self.reader.read_until(b'\n', buf)? == 0 {
			return Ok(false);
		}
		if !is_multiline_statement(buf) {
			return Ok(true);
		}

		let mut quotes = Quotes::default();
		let mut scanned = 0;
		loop {
			quotes.scan(&buf[scanned..]);
			scanned = buf.len();
			if !quotes.inside() && buf.trim_ascii_end().ends_with(b";") {
				return Ok(true);
			}

			// a truncated dump ends inside the statement, MySQL rejects it
			if self.reader.read_until(b'\n', buf)? == 0 {
				return Ok(true);
			}
		}
	}
}

fn is_multiline_statement(line: &[u8]) -> bool {
	["INSERT ", "REPLACE ", "CREATE TABLE "]
		.iter()
		.any(|keyword| starts_with_keyword(line, keyword))
}

fn starts_with_keyword(bytes: &[u8], keyword: &str) -> bool {
	bytes.len() >= keyword.len()
		&& bytes[..keyword.len()].eq_ignore_ascii_case(keyword.as_bytes())
}

/// Tracks whether a position is inside a string or a quoted identifier.
#[derive(Default)]
struct Quotes {
	quote: Option<u8>,
	escaped: bool,
}

impl Quotes {
	fn scan(&mut self, bytes: &[u8]) {
		for &byte in bytes {
			if self.escaped {
				self.escaped = false;
				continue;
			}

			match self.quote {
				// identifiers have no backslash escapes
				Some(quote) if byte == b'\\' && quote != b'`' => {
					self.escaped = true
				}
				Some(quote) if byte == quote => self.quote = None,
				Some(_) => {}
				None if matches!(byte, b'\'' | b'"' | b'`') => {
					self.quote = Some(byte)
				}
				None => {}
			}
		}
	}

	fn inside(&self) -> bool {
		self.quote.is_some()
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
	Cascade,
	SetNull,
	/// RESTRICT, NO ACTION or none: MySQL refuses the delete.
	Restrict,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ForeignKey {
	pub columns: Vec<String>,
	pub target: String,
	pub target_columns: Vec<String>,
	pub on_delete: OnDelete,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CreateTable {
	pub name: String,
	pub columns: Vec<String>,
	pub foreign_keys: Vec<ForeignKey>,
}

/// Parses a `CREATE TABLE` statement as mysqldump writes it: one column or
/// key per line.
pub fn parse_create_table(stmt: &[u8]) -> Option<CreateTable> {
	let stmt = String::from_utf8_lossy(stmt);
	let mut lines = stmt.lines();
	let name = identifiers(lines.next()?).pop()?;

	let mut columns = Vec::new();
	let mut foreign_keys = Vec::new();
	for line in lines {
		let line = line.trim();
		if line.starts_with('`') {
			columns.extend(identifiers(line).into_iter().next());
		} else if let Some(foreign_key) = parse_foreign_key(line) {
			foreign_keys.push(foreign_key);
		}
	}

	Some(CreateTable {
		name,
		columns,
		foreign_keys,
	})
}

/// `CONSTRAINT `x` FOREIGN KEY (`a`) REFERENCES `t` (`id`) ON DELETE
/// CASCADE`
fn parse_foreign_key(line: &str) -> Option<ForeignKey> {
	let (_, rest) = line.split_once("FOREIGN KEY")?;
	let (columns, rest) = rest.split_once("REFERENCES")?;
	let (target, rest) = rest.split_once('(')?;
	let (target_columns, actions) = rest.split_once(')')?;

	let on_delete = match actions.split_once("ON DELETE") {
		Some((_, action)) if action.trim_start().starts_with("CASCADE") => {
			OnDelete::Cascade
		}
		Some((_, action)) if action.trim_start().starts_with("SET NULL") => {
			OnDelete::SetNull
		}
		_ => OnDelete::Restrict,
	};

	Some(ForeignKey {
		columns: identifiers(columns),
		target: identifiers(target).pop()?,
		target_columns: identifiers(target_columns),
		on_delete,
	})
}

/// The backquoted identifiers in `text`.
fn identifiers(text: &str) -> Vec<String> {
	let mut identifiers = Vec::new();
	let mut chars = text.chars().peekable();
	while let Some(c) = chars.next() {
		if c != '`' {
			continue;
		}

		let mut identifier = String::new();
		while let Some(c) = chars.next() {
			// a doubled backquote is one inside the identifier
			if c == '`' && chars.next_if_eq(&'`').is_none() {
				break;
			}
			identifier.push(c);
		}
		identifiers.push(identifier);
	}

	identifiers
}

/// A parsed `INSERT ... VALUES (...),(...);`. Ranges point into the
/// statement.
#[derive(Debug, Default)]
pub struct Insert {
	pub table: String,
	/// The column list, when the statement has one.
	pub columns: Option<Vec<String>>,
	/// Everything up to and including `VALUES `.
	pub head: Range<usize>,
	/// Each row with its parentheses, and the range of its values in
	/// `values`.
	pub rows: Vec<(Range<usize>, Range<usize>)>,
	/// The values of all rows, as written (quotes, escapes, introducers).
	pub values: Vec<Range<usize>>,
}

impl Insert {
	pub fn row_values(&self, row: usize) -> &[Range<usize>] {
		&self.values[self.rows[row].1.clone()]
	}
}

pub fn parse_insert(stmt: &[u8]) -> Result<Insert, String> {
	let mut cursor = Cursor { stmt, pos: 0 };
	let mut insert = Insert::default();

	cursor.skip_whitespace();
	if !cursor.keyword("INSERT") && !cursor.keyword("REPLACE") {
		return Err("not an INSERT statement".into());
	}
	while !cursor.keyword("INTO") {
		// IGNORE, LOW_PRIORITY, ...
		if cursor.word().is_empty() {
			return Err("INSERT without INTO".into());
		}
	}
	insert.table = cursor.table_name()?;

	cursor.skip_whitespace();
	if cursor.peek() == Some(b'(') {
		cursor.pos += 1;
		let mut columns = Vec::new();
		loop {
			cursor.skip_whitespace();
			columns.push(cursor.identifier()?);
			cursor.skip_whitespace();
			match cursor.next_byte() {
				Some(b',') => continue,
				Some(b')') => break,
				_ => return Err("broken column list".into()),
			}
		}
		insert.columns = Some(columns);
	}

	if !cursor.keyword("VALUES") && !cursor.keyword("VALUE") {
		return Err("INSERT without VALUES".into());
	}
	cursor.skip_whitespace();
	insert.head = 0..cursor.pos;

	loop {
		cursor.skip_whitespace();
		let start = cursor.pos;
		if cursor.next_byte() != Some(b'(') {
			return Err(format!("expected a row at byte {start}"));
		}

		let first_value = insert.values.len();
		loop {
			cursor.skip_whitespace();
			insert.values.push(cursor.value()?);
			cursor.skip_whitespace();
			match cursor.next_byte() {
				Some(b',') => continue,
				Some(b')') => break,
				_ => {
					return Err(format!(
						"expected , or ) at byte {}",
						cursor.pos.saturating_sub(1)
					));
				}
			}
		}
		insert
			.rows
			.push((start..cursor.pos, first_value..insert.values.len()));

		cursor.skip_whitespace();
		match cursor.next_byte() {
			Some(b',') => continue,
			Some(b';') | None => break,
			_ => return Err("expected , or ; after a row".into()),
		}
	}

	Ok(insert)
}

struct Cursor<'a> {
	stmt: &'a [u8],
	pos: usize,
}

impl Cursor<'_> {
	fn peek(&self) -> Option<u8> {
		self.stmt.get(self.pos).copied()
	}

	fn next_byte(&mut self) -> Option<u8> {
		let byte = self.peek()?;
		self.pos += 1;
		Some(byte)
	}

	fn skip_whitespace(&mut self) {
		while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
			self.pos += 1;
		}
	}

	/// Consumes `keyword` and the whitespace after it if it comes next.
	fn keyword(&mut self, keyword: &str) -> bool {
		self.skip_whitespace();
		let rest = &self.stmt[self.pos..];
		let matches = starts_with_keyword(rest, keyword)
			&& !rest
				.get(keyword.len())
				.is_some_and(|&byte| is_word_byte(byte));
		if matches {
			self.pos += keyword.len();
		}

		matches
	}

	fn word(&mut self) -> &[u8] {
		self.skip_whitespace();
		let start = self.pos;
		while self.peek().is_some_and(is_word_byte) {
			self.pos += 1;
		}

		&self.stmt[start..self.pos]
	}

	/// `table` or `db`.`table`, returns the table.
	fn table_name(&mut self) -> Result<String, String> {
		self.skip_whitespace();
		let mut name = self.identifier()?;
		while self.peek() == Some(b'.') {
			self.pos += 1;
			name = self.identifier()?;
		}

		Ok(name)
	}

	fn identifier(&mut self) -> Result<String, String> {
		if self.peek() != Some(b'`') {
			let word = self.word();
			if word.is_empty() {
				return Err("expected an identifier".into());
			}
			return Ok(String::from_utf8_lossy(word).into_owned());
		}

		self.pos += 1;
		let mut name = Vec::new();
		loop {
			match self.next_byte() {
				Some(b'`') if self.peek() == Some(b'`') => {
					self.pos += 1;
					name.push(b'`');
				}
				Some(b'`') => break,
				Some(byte) => name.push(byte),
				None => return Err("unterminated identifier".into()),
			}
		}

		Ok(String::from_utf8_lossy(&name).into_owned())
	}

	/// A value as mysqldump writes it: `NULL`, a number, `0x..`, a string
	/// with an optional introducer (`_binary '...'`) or `X'..'`/`b'..'`.
	fn value(&mut self) -> Result<Range<usize>, String> {
		let start = self.pos;

		// an introducer followed by a string
		if self.peek() == Some(b'_') {
			let word_end = self.pos + self.word().len();
			self.skip_whitespace();
			if matches!(self.peek(), Some(b'\'' | b'"')) {
				self.string()?;
				return Ok(start..self.pos);
			}
			self.pos = word_end;
		}

		// X'..', b'..', N'..'
		if self.peek().is_some_and(|byte| b"xXbBnN".contains(&byte))
			&& self.stmt.get(self.pos + 1) == Some(&b'\'')
		{
			self.pos += 1;
		}
		if matches!(self.peek(), Some(b'\'' | b'"')) {
			self.string()?;
			return Ok(start..self.pos);
		}

		while self.peek().is_some_and(|byte| {
			!matches!(byte, b',' | b')') && !byte.is_ascii_whitespace()
		}) {
			self.pos += 1;
		}
		if self.pos == start {
			return Err(format!("expected a value at byte {start}"));
		}

		Ok(start..self.pos)
	}

	fn string(&mut self) -> Result<(), String> {
		let start = self.pos;
		let quote = self.next_byte();
		loop {
			match self.next_byte() {
				Some(b'\\') => {
					self.next_byte();
				}
				// a doubled quote is one inside the string
				byte if byte == quote && self.peek() == quote => {
					self.pos += 1;
				}
				byte if byte == quote => return Ok(()),
				Some(_) => {}
				None => {
					return Err(format!("unterminated string at byte {start}"));
				}
			}
		}
	}
}

fn is_word_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// What a value written as `raw` stands for: strings unescaped and without
/// introducer, numbers and hex literals as written, `None` for NULL.
pub fn decode(raw: &[u8]) -> Option<Cow<'_, [u8]>> {
	if raw.eq_ignore_ascii_case(b"NULL") {
		return None;
	}

	let mut string = raw;
	if string.first() == Some(&b'_') {
		let end = string
			.iter()
			.position(|byte| !is_word_byte(*byte))
			.unwrap_or(string.len());
		string = string[end..].trim_ascii_start();
	}

	match string {
		[quote @ (b'\'' | b'"'), inner @ .., last] if last == quote => {
			Some(Cow::Owned(unescape(inner, *quote)))
		}
		_ => Some(Cow::Borrowed(raw)),
	}
}

fn unescape(inner: &[u8], quote: u8) -> Vec<u8> {
	let mut out = Vec::with_capacity(inner.len());
	let mut bytes = inner.iter().copied();
	while let Some(byte) = bytes.next() {
		match byte {
			b'\\' => match bytes.next() {
				Some(b'0') => out.push(0),
				Some(b'b') => out.push(8),
				Some(b'n') => out.push(b'\n'),
				Some(b'r') => out.push(b'\r'),
				Some(b't') => out.push(b'\t'),
				Some(b'Z') => out.push(0x1a),
				// LIKE wildcards keep their backslash
				Some(byte @ (b'%' | b'_')) => out.extend([b'\\', byte]),
				Some(byte) => out.push(byte),
				None => out.push(b'\\'),
			},
			byte if byte == quote => {
				// the second of a doubled quote
				out.push(byte);
				bytes.next();
			}
			byte => out.push(byte),
		}
	}

	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn statements(dump: &str) -> Vec<String> {
		let mut reader = Statements::new(dump.as_bytes());
		let mut buf = Vec::new();
		let mut statements = Vec::new();
		while reader.next(&mut buf).unwrap() {
			statements.push(String::from_utf8(buf.clone()).unwrap());
		}
		statements
	}

	fn values(stmt: &str) -> Vec<Vec<Option<String>>> {
		let insert = parse_insert(stmt.as_bytes()).unwrap();
		(0..insert.rows.len())
			.map(|row| {
				insert
					.row_values(row)
					.iter()
					.map(|range| {
						decode(&stmt.as_bytes()[range.clone()]).map(|value| {
							String::from_utf8_lossy(&value).into_owned()
						})
					})
					.collect()
			})
			.collect()
	}

	fn s(value: &str) -> Option<String> {
		Some(value.to_string())
	}

	#[test]
	fn splits_statements() {
		let dump = "-- comment\n\
			CREATE TABLE `a` (\n  `id` int NOT NULL,\n  `x` text\n) ENGINE=InnoDB;\n\
			INSERT INTO `a` VALUES (1,'semi;\ncolon;'),(2,'it''s \\';\n');\n\
			UNLOCK TABLES;\n";

		assert_eq!(
			statements(dump),
			[
				"-- comment\n",
				"CREATE TABLE `a` (\n  `id` int NOT NULL,\n  `x` text\n) ENGINE=InnoDB;\n",
				"INSERT INTO `a` VALUES (1,'semi;\ncolon;'),(2,'it''s \\';\n');\n",
				"UNLOCK TABLES;\n",
			]
		);
	}

	#[test]
	fn parses_tricky_values() {
		let stmt = r#"INSERT INTO `t` VALUES (1,NULL,'a,b)c','it\'s','it''s','back\\slash\\',_binary 'bin\0\'',0x00FF,-1.5e3,'',"dq\"x",X'4142','\\'),(2,'(),',NULL,'','','','','','','','','');"#;

		let rows = values(stmt);
		assert_eq!(rows.len(), 2);
		assert_eq!(
			rows[0],
			[
				s("1"),
				None,
				s("a,b)c"),
				s("it's"),
				s("it's"),
				s("back\\slash\\"),
				s("bin\0'"),
				s("0x00FF"),
				s("-1.5e3"),
				s(""),
				s("dq\"x"),
				s("X'4142'"),
				s("\\"),
			]
		);
		assert_eq!(rows[1][0], s("2"));
		assert_eq!(rows[1][1], s("(),"));
		assert_eq!(rows[1][2], None);
	}

	#[test]
	fn keeps_row_text_and_head() {
		let stmt = "INSERT IGNORE INTO `db`.`t` (`id`, `name`) VALUES (1,'x'), ( 2 , 'y' );\n";
		let insert = parse_insert(stmt.as_bytes()).unwrap();

		assert_eq!(insert.table, "t");
		assert_eq!(insert.columns, Some(vec!["id".into(), "name".into()]));
		assert_eq!(
			&stmt[insert.head.clone()],
			"INSERT IGNORE INTO `db`.`t` (`id`, `name`) VALUES "
		);
		assert_eq!(&stmt[insert.rows[1].0.clone()], "( 2 , 'y' )");
		assert_eq!(&stmt[insert.row_values(1)[1].clone()], "'y'");
	}

	#[test]
	fn rejects_broken_inserts() {
		assert!(parse_insert(b"INSERT INTO `t` VALUES (1,'open);").is_err());
		assert!(parse_insert(b"INSERT INTO `t` VALUES 1,2;").is_err());
		assert!(parse_insert(b"UPDATE `t` SET x=1;").is_err());
	}

	#[test]
	fn parses_create_table() {
		let stmt = "CREATE TABLE `pre_addresses` (\n\
			  `id` int NOT NULL,\n\
			  `primaryOwnerId` int DEFAULT NULL,\n\
			  `we``ird` varchar(255) DEFAULT 'FOREIGN KEY',\n\
			  PRIMARY KEY (`id`),\n\
			  CONSTRAINT `fk_a` FOREIGN KEY (`id`) REFERENCES `pre_elements` (`id`) ON DELETE CASCADE,\n\
			  CONSTRAINT `fk_b` FOREIGN KEY (`primaryOwnerId`) REFERENCES `pre_elements` (`id`) ON DELETE SET NULL ON UPDATE CASCADE,\n\
			  CONSTRAINT `fk_c` FOREIGN KEY (`a`, `b`) REFERENCES `other` (`x`, `y`) ON UPDATE CASCADE\n\
			) ENGINE=InnoDB;\n";
		let table = parse_create_table(stmt.as_bytes()).unwrap();

		assert_eq!(table.name, "pre_addresses");
		assert_eq!(table.columns, ["id", "primaryOwnerId", "we`ird"]);
		assert_eq!(
			table.foreign_keys,
			[
				ForeignKey {
					columns: vec!["id".into()],
					target: "pre_elements".into(),
					target_columns: vec!["id".into()],
					on_delete: OnDelete::Cascade,
				},
				ForeignKey {
					columns: vec!["primaryOwnerId".into()],
					target: "pre_elements".into(),
					target_columns: vec!["id".into()],
					on_delete: OnDelete::SetNull,
				},
				ForeignKey {
					columns: vec!["a".into(), "b".into()],
					target: "other".into(),
					target_columns: vec!["x".into(), "y".into()],
					on_delete: OnDelete::Restrict,
				},
			]
		);
	}
}
