# craft-dbstrip

Removes personal data from a mysqldump of a [Craft CMS](https://craftcms.com)
site, on the dump itself, without a database, so a copy of a production
database can be used for development:

- Formie submissions, sent notifications and payments
- Commerce orders, line items, transactions, subscriptions and payment
  sources
- customers: every user that is not staff, with their addresses
- sessions, the queue, caches and tokens

Rows are dropped the way MySQL cascades a delete along the dump's foreign
keys, plus the element references Craft doesn't declare (`searchindex`,
`relations`, ...). References to dropped rows from kept rows are set to
`NULL`. It reads the dump three times and holds only the keys in memory,
so large dumps work. A table prefix (e.g. `craft_`) is detected.

```rust
// .sql, .sql.gz or a .sql.zip with one .sql in, the format by the output's
// extension out
let report = craft_dbstrip::strip_file(input, output)?;
println!("{report}");
if !report.craft {
	// the dump has no Craft `info` and `elements` tables
}
```

`craft_dbstrip::strip` works on any reader that can be opened again for
each pass.

## License

MIT or Apache-2.0, at your option.
