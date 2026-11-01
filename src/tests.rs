use super::*;

/// A small Craft 5 + Commerce + Formie dump with the table prefix `craft_`.
/// Elements: 1 admin, 2 editor (group), 3 customer, 4 entry, 5 order of
/// the customer, 6 address of the customer, 7 address of the order, 8 store
/// address, 9 submission, 10 sent notification, 11 asset uploaded with the
/// submission, 12 asset of the entry, 13 draft of the order, 14 user with
/// own permissions, 15 address of a deleted owner, 16 address element
/// without an address.
const DUMP: &str = r#"-- MySQL dump
CREATE TABLE `craft_info` (
  `id` int NOT NULL
) ENGINE=InnoDB;
CREATE TABLE `craft_drafts` (
  `id` int NOT NULL,
  `canonicalId` int DEFAULT NULL,
  `creatorId` int DEFAULT NULL,
  CONSTRAINT `fk_d1` FOREIGN KEY (`canonicalId`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE,
  CONSTRAINT `fk_d2` FOREIGN KEY (`creatorId`) REFERENCES `craft_users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB;
INSERT INTO `craft_drafts` VALUES (1,5,1);
CREATE TABLE `craft_elements` (
  `id` int NOT NULL,
  `canonicalId` int DEFAULT NULL,
  `draftId` int DEFAULT NULL,
  `type` varchar(255) NOT NULL,
  PRIMARY KEY (`id`),
  CONSTRAINT `fk_e1` FOREIGN KEY (`canonicalId`) REFERENCES `craft_elements` (`id`) ON DELETE SET NULL,
  CONSTRAINT `fk_e2` FOREIGN KEY (`draftId`) REFERENCES `craft_drafts` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_elements` VALUES (1,NULL,NULL,'craft\\elements\\User'),(2,NULL,NULL,'craft\\elements\\User'),(3,NULL,NULL,'craft\\elements\\User'),(4,NULL,NULL,'craft\\elements\\Entry'),(5,NULL,NULL,'craft\\commerce\\elements\\Order'),(6,NULL,NULL,'craft\\elements\\Address'),(7,NULL,NULL,'craft\\elements\\Address'),(8,NULL,NULL,'craft\\elements\\Address');
INSERT INTO `craft_elements` VALUES (9,NULL,NULL,'verbb\\formie\\elements\\Submission'),(10,NULL,NULL,'verbb\\formie\\elements\\SentNotification'),(11,NULL,NULL,'craft\\elements\\Asset'),(12,NULL,NULL,'craft\\elements\\Asset'),(13,5,1,'craft\\commerce\\elements\\Order'),(14,NULL,NULL,'craft\\elements\\User'),(15,NULL,NULL,'craft\\elements\\Address'),(16,NULL,NULL,'craft\\elements\\Address');
CREATE TABLE `craft_elements_sites` (
  `id` int NOT NULL,
  `elementId` int NOT NULL,
  `content` json DEFAULT NULL,
  CONSTRAINT `fk_es` FOREIGN KEY (`elementId`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_elements_sites` VALUES (1,1,NULL),(2,3,'{\"email\":\"cust@example.com\"}'),(3,4,'{\"title\":\"Entry, (with) \'quotes\'\"}'),(4,9,'{\"name\":\"Form Person\"}');
CREATE TABLE `craft_users` (
  `id` int NOT NULL,
  `admin` tinyint(1) NOT NULL DEFAULT '0',
  `email` varchar(255) DEFAULT NULL,
  CONSTRAINT `fk_u` FOREIGN KEY (`id`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_users` VALUES (1,1,'admin@example.org'),(2,0,'editor@example.org'),(3,0,'cust@example.com'),(14,0,'perm@example.org');
CREATE TABLE `craft_usergroups_users` (
  `id` int NOT NULL,
  `groupId` int NOT NULL,
  `userId` int NOT NULL,
  CONSTRAINT `fk_ug` FOREIGN KEY (`userId`) REFERENCES `craft_users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_usergroups_users` VALUES (1,1,2);
CREATE TABLE `craft_userpermissions_users` (
  `id` int NOT NULL,
  `permissionId` int NOT NULL,
  `userId` int NOT NULL,
  CONSTRAINT `fk_up` FOREIGN KEY (`userId`) REFERENCES `craft_users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_userpermissions_users` VALUES (1,1,14);
CREATE TABLE `craft_addresses` (
  `id` int NOT NULL,
  `primaryOwnerId` int DEFAULT NULL,
  `fullName` varchar(255) DEFAULT NULL,
  CONSTRAINT `fk_a1` FOREIGN KEY (`id`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE,
  CONSTRAINT `fk_a2` FOREIGN KEY (`primaryOwnerId`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_addresses` VALUES (6,3,'Customer Home'),(7,5,'Order Shipping'),(8,NULL,'Store'),(15,99,'Gone Owner');
CREATE TABLE `craft_commerce_orders` (
  `id` int NOT NULL,
  `email` varchar(255) DEFAULT NULL,
  `customerId` int DEFAULT NULL,
  CONSTRAINT `fk_o1` FOREIGN KEY (`id`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE,
  CONSTRAINT `fk_o2` FOREIGN KEY (`customerId`) REFERENCES `craft_elements` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB;
INSERT INTO `craft_commerce_orders` VALUES (5,'cust@example.com',3);
CREATE TABLE `craft_commerce_lineitems` (
  `id` int NOT NULL,
  `orderId` int NOT NULL,
  CONSTRAINT `fk_l` FOREIGN KEY (`orderId`) REFERENCES `craft_commerce_orders` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_commerce_lineitems` VALUES (1,5);
CREATE TABLE `craft_commerce_inventorytransactions` (
  `id` int NOT NULL,
  `lineItemId` int DEFAULT NULL,
  `userId` int DEFAULT NULL,
  CONSTRAINT `fk_it1` FOREIGN KEY (`lineItemId`) REFERENCES `craft_commerce_lineitems` (`id`) ON DELETE CASCADE,
  CONSTRAINT `fk_it2` FOREIGN KEY (`userId`) REFERENCES `craft_users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB;
INSERT INTO `craft_commerce_inventorytransactions` VALUES (1,1,NULL),(2,NULL,1),(3,NULL,3);
CREATE TABLE `craft_formie_submissions` (
  `id` int NOT NULL,
  `content` json DEFAULT NULL,
  CONSTRAINT `fk_s` FOREIGN KEY (`id`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_formie_submissions` VALUES (9,'{\"email\":\"form@example.com\"}');
CREATE TABLE `craft_formie_sentnotifications` (
  `id` int NOT NULL,
  `body` mediumtext,
  CONSTRAINT `fk_sn` FOREIGN KEY (`id`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_formie_sentnotifications` VALUES (10,'Hello Form Person');
CREATE TABLE `craft_relations` (
  `id` int NOT NULL,
  `sourceId` int NOT NULL,
  `targetId` int NOT NULL,
  CONSTRAINT `fk_r` FOREIGN KEY (`sourceId`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_relations` VALUES (1,9,11),(2,4,12),(3,4,3),(4,9,12);
CREATE TABLE `craft_assets` (
  `id` int NOT NULL,
  `filename` varchar(255) NOT NULL,
  CONSTRAINT `fk_as` FOREIGN KEY (`id`) REFERENCES `craft_elements` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB;
INSERT INTO `craft_assets` VALUES (11,'cv-form-person.pdf'),(12,'hero.jpg');
CREATE TABLE `craft_searchindex` (
  `elementId` int NOT NULL,
  `keywords` text NOT NULL
) ENGINE=MyISAM;
INSERT INTO `craft_searchindex` VALUES (16,' orphan street '),(1,' admin example org '),(3,' cust example com '),(4,' entry '),(9,' form example com ');
CREATE TABLE `craft_sessions` (
  `id` int NOT NULL,
  `token` char(100) NOT NULL
) ENGINE=InnoDB;
INSERT INTO `craft_sessions` VALUES (1,'secret');
CREATE TABLE `craft_fmc_contact` (
  `id` int NOT NULL,
  `field_email` text
) ENGINE=InnoDB;
INSERT INTO `craft_fmc_contact` VALUES (1,'old@example.com');
"#;

fn run(dump: &str) -> (String, Report) {
	let mut out = Vec::new();
	let report = strip(|| Ok(dump.as_bytes()), &mut out).unwrap();
	(String::from_utf8(out).unwrap(), report)
}

fn rows<'a>(out: &'a str, table: &str) -> Vec<&'a str> {
	let head = format!("INSERT INTO `{table}` VALUES ");
	out.lines()
		.filter_map(|line| line.strip_prefix(&head))
		.flat_map(|values| {
			let values = values.trim_end_matches(';');
			values
				.strip_prefix('(')
				.unwrap()
				.strip_suffix(')')
				.unwrap()
				.split("),(")
		})
		.collect()
}

fn ids<'a>(out: &'a str, table: &str) -> Vec<&'a str> {
	rows(out, table)
		.into_iter()
		.map(|row| row.split(',').next().unwrap())
		.collect()
}

#[test]
fn removes_customers_orders_and_submissions() {
	let (out, report) = run(DUMP);
	assert!(report.craft);

	assert_eq!(
		ids(&out, "craft_elements"),
		["1", "2", "4", "8", "12", "14"]
	);
	assert_eq!(ids(&out, "craft_users"), ["1", "2", "14"]);
	assert_eq!(ids(&out, "craft_addresses"), ["8"]);
	assert_eq!(ids(&out, "craft_elements_sites"), ["1", "3"]);
	assert_eq!(ids(&out, "craft_relations"), ["2"]);
	assert_eq!(ids(&out, "craft_assets"), ["12"]);
	assert_eq!(ids(&out, "craft_searchindex"), ["1", "4"]);
	// 3 loses its customer, the user who counted stock stays
	assert_eq!(
		rows(&out, "craft_commerce_inventorytransactions"),
		["2,NULL,1", "3,NULL,NULL"]
	);
	for table in [
		"craft_drafts",
		"craft_commerce_orders",
		"craft_commerce_lineitems",
		"craft_formie_submissions",
		"craft_formie_sentnotifications",
		"craft_sessions",
		"craft_fmc_contact",
	] {
		assert!(rows(&out, table).is_empty(), "{table} is not empty");
		assert!(!out.contains(&format!("INSERT INTO `{table}`")));
	}
	for personal in [
		"example.com",
		"Form Person",
		"Customer Home",
		"Gone Owner",
		"cv-form",
		"orphan street",
	] {
		assert!(!out.contains(personal), "{personal} is still there");
	}

	// kept rows as they were, the rest of the dump too
	assert!(
		out.contains(r#"(3,4,'{\"title\":\"Entry, (with) \'quotes\'\"}')"#)
	);
	assert!(out.contains("CREATE TABLE `craft_sessions`"));
	assert!(out.starts_with("-- MySQL dump\n"));

	assert_eq!(report.tables["craft_users"].dropped, 1);
	assert_eq!(report.tables["craft_elements"].dropped, 10);
}

#[test]
fn sets_references_to_removed_rows_to_null() {
	let dump = DUMP.replace(
		"INSERT INTO `craft_commerce_inventorytransactions` VALUES (1,1,NULL),(2,NULL,1),(3,NULL,3);",
		"INSERT INTO `craft_commerce_inventorytransactions` VALUES (1,1,NULL),(2,NULL,1),(3,NULL,3),(4,NULL,2);",
	);
	let dump = dump.replace(
		"CONSTRAINT `fk_it1` FOREIGN KEY (`lineItemId`) REFERENCES `craft_commerce_lineitems` (`id`) ON DELETE CASCADE,",
		"CONSTRAINT `fk_it1` FOREIGN KEY (`lineItemId`) REFERENCES `craft_commerce_lineitems` (`id`) ON DELETE SET NULL,",
	);
	let (out, report) = run(&dump);

	// 1 loses its line item, 3 its customer, 2 and 4 refer to staff
	assert_eq!(
		rows(&out, "craft_commerce_inventorytransactions"),
		["1,NULL,NULL", "2,NULL,1", "3,NULL,NULL", "4,NULL,2"]
	);
	assert_eq!(
		report.tables["craft_commerce_inventorytransactions"].nulled,
		2
	);
}

#[test]
fn reads_column_lists() {
	let dump = DUMP.replace(
		"INSERT INTO `craft_users` VALUES (1,1,'admin@example.org'),(2,0,'editor@example.org'),(3,0,'cust@example.com'),(14,0,'perm@example.org');",
		"INSERT INTO `craft_users` (`email`, `id`, `admin`) VALUES ('admin@example.org',1,1),('editor@example.org',2,0),('cust@example.com',3,0),('perm@example.org',14,0);",
	);
	let (out, _) = run(&dump);

	assert!(out.contains(
		"INSERT INTO `craft_users` (`email`, `id`, `admin`) VALUES ('admin@example.org',1,1),('editor@example.org',2,0),('perm@example.org',14,0);"
	));
}

#[test]
fn passes_other_databases_through() {
	let dump = "CREATE TABLE `posts` (\n  `id` int NOT NULL,\n  `body` text\n) ENGINE=InnoDB;\n\
		INSERT INTO `posts` VALUES (1,'a;b'),(2,'c');\n";
	let (out, report) = run(dump);

	assert_eq!(out, dump);
	assert_eq!(report.dropped(), 0);
	assert!(!report.craft);
}

#[test]
fn refuses_what_is_no_dump() {
	let mut out = Vec::new();
	for input in [&b"garbage\n"[..], &[0x1f, 0x8b, 8, 0, 1, 2, 3][..], b""] {
		assert!(strip(|| Ok(input), &mut out).is_err());
	}

	// a data-only dump would pass through unchecked
	let error = strip(
		|| {
			Ok(&b"INSERT INTO `craft_users` VALUES (3,0,'cust@example.com');\n"[..])
		},
		&mut out,
	)
	.unwrap_err();
	assert!(error.contains("CREATE TABLE"), "{error}");
}

/// A new, empty directory for file tests.
fn temp_dir(name: &str) -> std::path::PathBuf {
	let dir = std::env::temp_dir()
		.join(format!("craft-dbstrip-{name}-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

fn gzip(data: &[u8]) -> Vec<u8> {
	let mut encoder =
		flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
	encoder.write_all(data).unwrap();
	encoder.finish().unwrap()
}

fn zip(name: &str, data: &[u8]) -> Vec<u8> {
	let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
	zip.start_file(name, zip::write::SimpleFileOptions::default())
		.unwrap();
	zip.write_all(data).unwrap();
	zip.finish().unwrap().into_inner()
}

#[test]
fn detects_archives_by_content_not_name() {
	let dir = temp_dir("formats");
	let (expected, _) = run(DUMP);

	// gzip and zip under misleading names, written as zip and plain SQL
	for (name, data) in [
		("gzip.sql", gzip(DUMP.as_bytes())),
		("zip.sql.gz", zip("dump.sql", DUMP.as_bytes())),
		("plain.zip", DUMP.as_bytes().to_vec()),
	] {
		let input = dir.join(name);
		std::fs::write(&input, data).unwrap();

		let output = dir.join(format!("{name}.out.sql"));
		let report = strip_file(&input, &output).unwrap();
		assert!(report.craft);
		assert_eq!(
			std::fs::read_to_string(&output).unwrap(),
			expected,
			"{name}"
		);

		let output = dir.join(format!("{name}.out.sql.zip"));
		strip_file(&input, &output).unwrap();
		let mut archive =
			zip::ZipArchive::new(std::fs::File::open(&output).unwrap())
				.unwrap();
		let mut sql = String::new();
		io::Read::read_to_string(&mut archive.by_index(0).unwrap(), &mut sql)
			.unwrap();
		assert_eq!(sql, expected, "{name}");
	}

	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn refuses_nested_archives_and_garbage_files() {
	let dir = temp_dir("refuses");

	for (name, data) in [
		("double.sql.gz", gzip(&gzip(DUMP.as_bytes()))),
		(
			"gzip-in-zip.zip",
			zip("dump.sql.gz", &gzip(DUMP.as_bytes())),
		),
		("garbage.sql", b"garbage".to_vec()),
	] {
		let input = dir.join(name);
		std::fs::write(&input, data).unwrap();

		let output = dir.join(format!("{name}.out.sql.zip"));
		assert!(strip_file(&input, &output).is_err(), "{name}");
		assert!(!output.exists(), "{name}");
	}

	// nothing left behind but the inputs
	assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn fails_on_broken_inserts() {
	let dump = format!("{DUMP}INSERT INTO `craft_users` VALUES (1,'open);\n");
	let mut out = Vec::new();
	let error = strip(|| Ok(dump.as_bytes()), &mut out).unwrap_err();

	assert!(error.contains("craft_users"), "{error}");
	assert!(!error.contains("open"), "{error}");
}
