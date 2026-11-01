//! Removes personal data of forms and the shop from a mysqldump of a Craft
//! site, on the dump file itself, without a database: Formie submissions
//! and sent notifications, Commerce orders, subscriptions, payment data and
//! customers (every user that is not staff), the addresses of all of them,
//! and sessions, queue jobs and tokens.
//!
//! Three passes over the dump: the first reads the `CREATE TABLE`
//! statements, the second the key columns of every row. Then the rows to
//! drop are worked out the way MySQL cascades a delete along the dump's
//! foreign keys (plus the element references Craft doesn't declare, like
//! `searchindex`), and the third pass writes the dump without them.

mod files;
mod sql;

use std::{
	collections::{BTreeMap, HashMap, HashSet},
	fmt,
	io::{self, BufRead, Write},
};

pub use files::strip_file;
use sql::{OnDelete, Statements, parse_create_table, parse_insert};

/// Elements that are personal data, with everything that belongs to them.
const REMOVED_ELEMENT_TYPES: &[&str] = &[
	"verbb\\formie\\elements\\Submission",
	"verbb\\formie\\elements\\SentNotification",
	"verbb\\formie\\elements\\NestedFieldRow",
	"craft\\commerce\\elements\\Order",
	"craft\\commerce\\elements\\Subscription",
];

/// Tables that lose all rows (without the table prefix).
const EMPTIED_TABLES: &[&str] = &[
	"sessions",
	"phpsessions",
	"queue",
	"cache",
	"tokens",
	"formie_submissions",
	"formie_sentnotifications",
	"formie_nestedfieldrows",
	"formie_payments",
	"formie_payments_subscriptions",
	"commerce_orders",
	"commerce_lineitems",
	"commerce_orderadjustments",
	"commerce_orderhistories",
	"commerce_ordernotices",
	"commerce_transactions",
	"commerce_paymentsources",
	"commerce_subscriptions",
	"commerce_customer_discountuses",
	"commerce_email_discountuses",
];

/// Formie's content tables on Craft 4, one per form.
const EMPTIED_TABLE_PREFIXES: &[&str] = &["fmc_"];

/// References Craft doesn't declare as foreign keys, followed like `ON
/// DELETE CASCADE`: (table, column, target table). They replace a declared
/// foreign key on the same column.
const UNDECLARED_REFERENCES: &[(&str, &str, &str)] = &[
	("searchindex", "elementId", "elements"),
	("searchindexqueue", "elementId", "elements"),
	("relations", "targetId", "elements"),
	("structureelements", "elementId", "elements"),
	("elements_bulkops", "elementId", "elements"),
	("imagetransformindex", "assetId", "assets"),
	(
		"commerce_orderadjustments",
		"lineItemId",
		"commerce_lineitems",
	),
	// Commerce 3: customers are not users, a customer goes with its user
	("commerce_customers", "userId", "users"),
];

/// Tables whose `sourceId` relates to elements in `targetId`.
const RELATION_TABLES: &[&str] = &["relations", "formie_relations"];

/// Removes the personal data from the dump that `open` reads and writes the
/// rest to `out`. `open` is called once per pass.
pub fn strip<R: BufRead>(
	open: impl Fn() -> io::Result<R>,
	out: &mut impl Write,
) -> Result<Report, String> {
	let read_error = |e: io::Error| format!("Failed to read the dump: {e}");

	let mut dump = Dump::default();
	for_each_statement(open().map_err(read_error)?, |stmt| {
		if let Some(table) = is_create_table(stmt)
			.then(|| parse_create_table(stmt))
			.flatten()
		{
			dump.add_table(table);
		}
		Ok(())
	})?;
	// anything else would pass through unchanged
	if dump.tables.is_empty() {
		return Err("the dump has no CREATE TABLE statement, it is not a mysqldump (or still compressed)".into());
	}
	dump.resolve();

	for_each_statement(open().map_err(read_error)?, |stmt| {
		if is_insert(stmt) {
			dump.read_rows(stmt)?;
		}
		Ok(())
	})?;
	dump.mark();

	let mut report = Report {
		craft: dump.table("info").is_some() && dump.table("elements").is_some(),
		..Report::default()
	};
	let mut written = vec![0; dump.tables.len()];
	for_each_statement(open().map_err(read_error)?, |stmt| {
		let result = if is_insert(stmt) {
			dump.write_insert(stmt, out, &mut written, &mut report)
		} else {
			out.write_all(stmt).map_err(|e| e.to_string())
		};
		result.map_err(|e| format!("Failed to write the stripped dump: {e}"))
	})?;

	Ok(report)
}

fn for_each_statement(
	reader: impl BufRead,
	mut f: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
	let mut statements = Statements::new(reader);
	let mut buf = Vec::new();
	while statements
		.next(&mut buf)
		.map_err(|e| format!("Failed to read the dump: {e}"))?
	{
		f(&buf)?;
	}

	Ok(())
}

fn is_create_table(stmt: &[u8]) -> bool {
	stmt.starts_with(b"CREATE TABLE ")
}

fn is_insert(stmt: &[u8]) -> bool {
	stmt.starts_with(b"INSERT ") || stmt.starts_with(b"REPLACE ")
}

/// How many rows the strip dropped or changed, per table.
#[derive(Debug, Default)]
pub struct Report {
	/// Whether the dump is a Craft database (has `info` and `elements`).
	pub craft: bool,
	pub tables: BTreeMap<String, TableReport>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TableReport {
	pub dropped: u64,
	/// Rows kept with a reference to a dropped row set to NULL.
	pub nulled: u64,
}

impl Report {
	pub fn dropped(&self) -> u64 {
		self.tables.values().map(|table| table.dropped).sum()
	}
}

impl fmt::Display for Report {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} rows dropped", self.dropped())?;
		for (name, table) in &self.tables {
			write!(f, "\n  {name}: {} dropped", table.dropped)?;
			if table.nulled > 0 {
				write!(f, ", {} with a reference set to NULL", table.nulled)?;
			}
		}
		Ok(())
	}
}

/// A value as a key: decoded, columns of a composite key joined by a 0 byte.
type Key = Box<[u8]>;

#[derive(Default)]
struct Dump {
	tables: Vec<Table>,
	by_name: HashMap<String, usize>,
	/// Foreign keys by table, until `resolve`.
	foreign_keys: Vec<(usize, sql::ForeignKey)>,
	references: Vec<Reference>,
	prefix: String,
}

struct Table {
	name: String,
	columns: Vec<String>,
	emptied: bool,
	/// Columns whose values the second pass keeps.
	tracked: Vec<usize>,
	/// The tracked values of every row.
	rows: Vec<Vec<Option<Key>>>,
	dropped: Vec<bool>,
	/// Columns set to NULL, by row.
	nulled: HashMap<usize, Vec<usize>>,
}

/// A foreign key: a row of `table` refers to the row of `target` with the
/// same values.
struct Reference {
	table: usize,
	columns: Vec<usize>,
	target: usize,
	target_columns: Vec<usize>,
	/// Whether a row goes with its target or only loses the reference.
	cascade: bool,
}

impl Dump {
	fn add_table(&mut self, create: sql::CreateTable) {
		// a second definition of a table: its rows are counted as one
		if self.by_name.contains_key(&create.name) {
			return;
		}

		let index = self.tables.len();
		self.by_name.insert(create.name.clone(), index);
		for foreign_key in create.foreign_keys {
			self.foreign_keys.push((index, foreign_key));
		}
		self.tables.push(Table {
			name: create.name,
			columns: create.columns,
			emptied: false,
			tracked: Vec::new(),
			rows: Vec::new(),
			dropped: Vec::new(),
			nulled: HashMap::new(),
		});
	}

	fn table(&self, name: &str) -> Option<usize> {
		self.by_name.get(&format!("{}{name}", self.prefix)).copied()
	}

	fn column(&self, table: usize, name: &str) -> Option<usize> {
		self.tables[table].columns.iter().position(|c| c == name)
	}

	/// Finds the table prefix and the references, and decides which columns
	/// the second pass keeps.
	fn resolve(&mut self) {
		// Craft always has `info` and `elements`
		self.prefix = self
			.tables
			.iter()
			.filter_map(|table| table.name.strip_suffix("elements"))
			.find(|prefix| self.by_name.contains_key(&format!("{prefix}info")))
			.unwrap_or_default()
			.to_string();

		for table in &mut self.tables {
			let Some(name) = table.name.strip_prefix(&self.prefix) else {
				continue;
			};
			table.emptied = EMPTIED_TABLES.contains(&name)
				|| EMPTIED_TABLE_PREFIXES
					.iter()
					.any(|prefix| name.starts_with(prefix));
		}

		for (table, foreign_key) in std::mem::take(&mut self.foreign_keys) {
			let Some(target) = self.by_name.get(&foreign_key.target).copied()
			else {
				continue;
			};
			let columns: Option<Vec<_>> = foreign_key
				.columns
				.iter()
				.map(|column| self.column(table, column))
				.collect();
			let target_columns: Option<Vec<_>> = foreign_key
				.target_columns
				.iter()
				.map(|column| self.column(target, column))
				.collect();
			let (Some(columns), Some(target_columns)) =
				(columns, target_columns)
			else {
				continue;
			};

			self.references.push(Reference {
				table,
				columns,
				target,
				target_columns,
				// a row that would block the delete in MySQL goes too
				cascade: foreign_key.on_delete != OnDelete::SetNull,
			});
		}

		for (table, column, target) in UNDECLARED_REFERENCES {
			let (Some(table), Some(target)) =
				(self.table(table), self.table(target))
			else {
				continue;
			};
			let (Some(column), Some(id)) =
				(self.column(table, column), self.column(target, "id"))
			else {
				continue;
			};

			self.references.retain(|reference| {
				reference.table != table || reference.columns != [column]
			});
			self.references.push(Reference {
				table,
				columns: vec![column],
				target,
				target_columns: vec![id],
				cascade: true,
			});
		}

		let mut tracked: Vec<(usize, usize)> = Vec::new();
		for reference in &self.references {
			tracked.extend(
				reference.columns.iter().map(|c| (reference.table, *c)),
			);
			tracked.extend(
				reference
					.target_columns
					.iter()
					.map(|c| (reference.target, *c)),
			);
		}
		for (table, column) in [
			("elements", "id"),
			("elements", "type"),
			("users", "id"),
			("users", "admin"),
			("addresses", "primaryOwnerId"),
			("commerce_customers", "userId"),
			("commerce_addresses", "isStoreLocation"),
		] {
			if let Some(table) = self.table(table)
				&& let Some(column) = self.column(table, column)
			{
				tracked.push((table, column));
			}
		}
		for (table, column) in tracked {
			let tracked = &mut self.tables[table].tracked;
			if !tracked.contains(&column) {
				tracked.push(column);
			}
		}
	}

	/// Second pass: keeps the tracked values of the rows of an INSERT.
	fn read_rows(&mut self, stmt: &[u8]) -> Result<(), String> {
		let insert = parse_insert(stmt).map_err(|e| insert_error(stmt, e))?;
		// its rows would pass through unchecked
		let Some(&index) = self.by_name.get(&insert.table) else {
			return Err(format!(
				"the dump inserts into `{}` without its CREATE TABLE",
				insert.table
			));
		};
		let table = &mut self.tables[index];
		if table.tracked.is_empty() {
			return Ok(());
		}

		let positions = value_positions(table, insert.columns.as_deref());
		for row in 0..insert.rows.len() {
			let values = insert.row_values(row);
			let tracked = table
				.tracked
				.iter()
				.map(|&column| {
					let range = values.get(positions[column]?)?;
					sql::decode(&stmt[range.clone()]).map(|v| v.into())
				})
				.collect();
			table.rows.push(tracked);
		}

		Ok(())
	}

	fn value(&self, table: usize, row: usize, column: usize) -> Option<&[u8]> {
		let table = &self.tables[table];
		let slot = table.tracked.iter().position(|c| *c == column)?;
		table.rows[row][slot].as_deref()
	}

	/// The key of `row` in `columns`, None if one of them is NULL.
	fn key(&self, table: usize, row: usize, columns: &[usize]) -> Option<Key> {
		let mut key = Vec::new();
		for (i, &column) in columns.iter().enumerate() {
			if i > 0 {
				key.push(0);
			}
			key.extend_from_slice(self.value(table, row, column)?);
		}

		Some(key.into())
	}

	fn ids(&self, table: &str, column: &str) -> HashSet<Key> {
		let Some(table) = self.table(table) else {
			return HashSet::new();
		};
		let Some(column) = self.column(table, column) else {
			return HashSet::new();
		};

		(0..self.tables[table].rows.len())
			.filter_map(|row| self.key(table, row, &[column]))
			.collect()
	}

	/// Works out which rows go and which references become NULL.
	fn mark(&mut self) {
		for table in &mut self.tables {
			table.dropped = vec![table.emptied; table.rows.len()];
		}

		self.drop_personal_rows();
		self.cascade();
		self.drop_orphaned_assets();
		self.cascade();
		self.null_references();
	}

	fn drop_personal_rows(&mut self) {
		if let Some(elements) = self.table("elements")
			&& let Some(column) = self.column(elements, "type")
		{
			for row in 0..self.tables[elements].rows.len() {
				let removed =
					self.value(elements, row, column).is_some_and(|t| {
						REMOVED_ELEMENT_TYPES.iter().any(|r| r.as_bytes() == t)
					});
				if removed {
					self.tables[elements].dropped[row] = true;
				}
			}
		}

		// customers: users that are not admins, in no group and have no
		// permissions of their own
		if let Some(users) = self.table("users")
			&& let Some(id) = self.column(users, "id")
		{
			let mut staff = self.ids("usergroups_users", "userId");
			staff.extend(self.ids("userpermissions_users", "userId"));
			let admin = self.column(users, "admin");

			for row in 0..self.tables[users].rows.len() {
				let is_admin = admin.is_some_and(|admin| {
					self.value(users, row, admin) == Some(b"1")
				});
				let is_staff = is_admin
					|| self
						.key(users, row, &[id])
						.is_some_and(|id| staff.contains(&id));
				if !is_staff {
					self.tables[users].dropped[row] = true;
				}
			}
		}

		// addresses of an owner that no longer exists
		if let Some(addresses) = self.table("addresses")
			&& let Some(owner) = self.column(addresses, "primaryOwnerId")
		{
			let elements = self.ids("elements", "id");
			for row in 0..self.tables[addresses].rows.len() {
				if self
					.key(addresses, row, &[owner])
					.is_some_and(|owner| !elements.contains(&owner))
				{
					self.tables[addresses].dropped[row] = true;
				}
			}
		}

		// address elements without an address, whose search index still
		// has it
		if let Some(elements) = self.table("elements")
			&& let (Some(id), Some(column)) =
				(self.column(elements, "id"), self.column(elements, "type"))
			&& self.table("addresses").is_some()
		{
			let addresses = self.ids("addresses", "id");
			for row in 0..self.tables[elements].rows.len() {
				let is_address = self.value(elements, row, column)
					== Some(b"craft\\elements\\Address");
				if is_address
					&& self
						.key(elements, row, &[id])
						.is_some_and(|id| !addresses.contains(&id))
				{
					self.tables[elements].dropped[row] = true;
				}
			}
		}

		// Commerce 3: guest customers and every address but the store's
		for (table, column, keep) in [
			("commerce_customers", "userId", None),
			("commerce_addresses", "isStoreLocation", Some(&b"1"[..])),
		] {
			let Some(table) = self.table(table) else {
				continue;
			};
			let Some(column) = self.column(table, column) else {
				continue;
			};
			for row in 0..self.tables[table].rows.len() {
				let value = self.value(table, row, column);
				let goes = match keep {
					None => value.is_none(),
					Some(keep) => value != Some(keep),
				};
				if goes {
					self.tables[table].dropped[row] = true;
				}
			}
		}
	}

	/// Drops every row that refers to a dropped row, until nothing changes.
	/// A row of an element's own table (`users`, `addresses`, ...) takes
	/// its element along.
	fn cascade(&mut self) {
		let elements = self.table("elements");
		let element_ids = elements
			.and_then(|elements| Some((elements, self.column(elements, "id")?)))
			.map(|(elements, id)| {
				let ids: HashMap<Key, usize> =
					(0..self.tables[elements].rows.len())
						.filter_map(|row| {
							Some((self.key(elements, row, &[id])?, row))
						})
						.collect();
				(elements, ids)
			});
		let element_tables: Vec<(usize, usize)> = self
			.references
			.iter()
			.filter(|r| {
				Some(r.target) == elements
					&& r.table != r.target
					&& r.columns.len() == 1
					&& self.tables[r.table].columns[r.columns[0]] == "id"
			})
			.map(|r| (r.table, r.columns[0]))
			.collect();

		loop {
			let mut changed = false;

			// per target, rows dropped in this round count in the next
			let mut dropped_keys = HashMap::new();
			for reference in 0..self.references.len() {
				let Reference {
					table,
					ref columns,
					target,
					ref target_columns,
					cascade,
				} = self.references[reference];
				if !cascade {
					continue;
				}

				let dropped = dropped_keys
					.entry((target, target_columns.clone()))
					.or_insert_with(|| {
						self.dropped_keys(target, target_columns)
					});
				if dropped.is_empty() {
					continue;
				}
				for row in 0..self.tables[table].rows.len() {
					if !self.tables[table].dropped[row]
						&& self
							.key(table, row, columns)
							.is_some_and(|key| dropped.contains(&key))
					{
						self.tables[table].dropped[row] = true;
						changed = true;
					}
				}
			}

			if let Some((elements, ids)) = &element_ids {
				for &(table, id) in &element_tables {
					for row in 0..self.tables[table].rows.len() {
						if !self.tables[table].dropped[row] {
							continue;
						}
						let Some(element) = self
							.key(table, row, &[id])
							.and_then(|id| ids.get(&id).copied())
						else {
							continue;
						};
						if !self.tables[*elements].dropped[element] {
							self.tables[*elements].dropped[element] = true;
							changed = true;
						}
					}
				}
			}

			if !changed {
				break;
			}
		}
	}

	fn dropped_keys(&self, table: usize, columns: &[usize]) -> HashSet<Key> {
		(0..self.tables[table].rows.len())
			.filter(|&row| self.tables[table].dropped[row])
			.filter_map(|row| self.key(table, row, columns))
			.collect()
	}

	/// Drops assets that only dropped elements relate to, like files
	/// uploaded with a form.
	fn drop_orphaned_assets(&mut self) {
		let Some(elements) = self.table("elements") else {
			return;
		};
		let Some(id) = self.column(elements, "id") else {
			return;
		};
		let assets = self.ids("assets", "id");

		let mut orphaned = HashSet::new();
		let mut used = HashSet::new();
		for name in RELATION_TABLES {
			let Some(table) = self.table(name) else {
				continue;
			};
			let Some(target) = self.column(table, "targetId") else {
				continue;
			};
			for row in 0..self.tables[table].rows.len() {
				let Some(key) = self.key(table, row, &[target]) else {
					continue;
				};
				if !assets.contains(&key) {
					continue;
				}

				if self.tables[table].dropped[row] {
					orphaned.insert(key);
				} else {
					used.insert(key);
				}
			}
		}

		for row in 0..self.tables[elements].rows.len() {
			if self
				.key(elements, row, &[id])
				.is_some_and(|id| orphaned.contains(&id) && !used.contains(&id))
			{
				self.tables[elements].dropped[row] = true;
			}
		}
	}

	fn null_references(&mut self) {
		for reference in 0..self.references.len() {
			let Reference {
				table,
				ref columns,
				target,
				ref target_columns,
				cascade,
			} = self.references[reference];
			if cascade {
				continue;
			}

			let dropped = self.dropped_keys(target, target_columns);
			if dropped.is_empty() {
				continue;
			}
			for row in 0..self.tables[table].rows.len() {
				if !self.tables[table].dropped[row]
					&& self
						.key(table, row, columns)
						.is_some_and(|key| dropped.contains(&key))
				{
					let columns = columns.clone();
					self.tables[table]
						.nulled
						.entry(row)
						.or_default()
						.extend(columns);
				}
			}
		}
	}

	/// Third pass: writes an INSERT without its dropped rows, or nothing if
	/// none is left.
	fn write_insert(
		&self,
		stmt: &[u8],
		out: &mut impl Write,
		written: &mut [usize],
		report: &mut Report,
	) -> Result<(), String> {
		let insert = parse_insert(stmt).map_err(|e| insert_error(stmt, e))?;
		let Some(&index) = self.by_name.get(&insert.table) else {
			return out.write_all(stmt).map_err(|e| e.to_string());
		};
		let table = &self.tables[index];
		let changed = table.emptied
			|| table.dropped.contains(&true)
			|| !table.nulled.is_empty();
		if !changed {
			return out.write_all(stmt).map_err(|e| e.to_string());
		}

		let positions = value_positions(table, insert.columns.as_deref());
		let table_report = report.tables.entry(table.name.clone()).or_default();
		let mut kept = Vec::new();
		for row in 0..insert.rows.len() {
			let index_in_table = written[index];
			written[index] += 1;

			let dropped = table.emptied
				|| table.dropped.get(index_in_table).copied().unwrap_or(false);
			if dropped {
				table_report.dropped += 1;
				continue;
			}

			let Some(nulled) = table.nulled.get(&index_in_table) else {
				kept.push(stmt[insert.rows[row].0.clone()].to_vec());
				continue;
			};
			table_report.nulled += 1;
			let nulled: Vec<usize> =
				nulled.iter().filter_map(|&c| positions[c]).collect();
			let values: Vec<&[u8]> = insert
				.row_values(row)
				.iter()
				.enumerate()
				.map(|(i, range)| {
					if nulled.contains(&i) {
						&b"NULL"[..]
					} else {
						&stmt[range.clone()]
					}
				})
				.collect();
			let mut text = b"(".to_vec();
			text.extend(values.join(&b","[..]));
			text.push(b')');
			kept.push(text);
		}

		if kept.is_empty() {
			return Ok(());
		}
		let tail = insert.rows.last().map_or(stmt.len(), |row| row.0.end);
		let mut text = stmt[insert.head.clone()].to_vec();
		text.extend(kept.join(&b","[..]));
		text.extend_from_slice(&stmt[tail..]);

		out.write_all(&text).map_err(|e| e.to_string())
	}
}

/// Where each column of `table` is among the values of an INSERT with
/// `columns` (or all columns in order).
fn value_positions(
	table: &Table,
	columns: Option<&[String]>,
) -> Vec<Option<usize>> {
	match columns {
		Some(columns) => table
			.columns
			.iter()
			.map(|name| columns.iter().position(|c| c == name))
			.collect(),
		None => (0..table.columns.len()).map(Some).collect(),
	}
}

/// Names the statement without its values, which are personal data.
fn insert_error(stmt: &[u8], error: String) -> String {
	let head = stmt
		.windows(7)
		.position(|w| w.eq_ignore_ascii_case(b" VALUES"))
		.unwrap_or(0)
		.min(100);
	let head = String::from_utf8_lossy(&stmt[..head]);
	format!("Failed to parse `{head} VALUES ...`: {error}")
}

#[cfg(test)]
mod tests;
