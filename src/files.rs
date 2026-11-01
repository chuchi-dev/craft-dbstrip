//! Dumps as files: `.sql`, `.sql.gz` or a `.sql.zip` with one `.sql`, the
//! way hosting panels hand them out.

use std::{
	fs::{self, File},
	io::{self, BufRead, BufReader, BufWriter, Write},
	path::{Path, PathBuf},
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::{Report, strip};

#[derive(Clone, Copy)]
enum Format {
	Sql,
	Gzip,
	Zip,
}

impl Format {
	/// The format of a file by its first bytes, never its name: a browser or
	/// a hosting panel may name a download anything.
	fn sniff(path: &Path) -> Result<Format, String> {
		let file = File::open(path)
			.map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
		let mut reader = BufReader::new(file);
		let start = reader
			.fill_buf()
			.map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

		Ok(Format::of_bytes(start))
	}

	fn of_bytes(start: &[u8]) -> Format {
		if start.starts_with(&[0x1f, 0x8b]) {
			Format::Gzip
		} else if start.starts_with(b"PK\x03\x04") {
			Format::Zip
		} else {
			Format::Sql
		}
	}

	/// The format to write, by the output's name.
	fn of(path: &Path) -> Format {
		match path.extension().and_then(|e| e.to_str()) {
			Some(e) if e.eq_ignore_ascii_case("gz") => Format::Gzip,
			Some(e) if e.eq_ignore_ascii_case("zip") => Format::Zip,
			_ => Format::Sql,
		}
	}
}

/// Strips the dump `input` into `output`, both in the format their
/// extension names. `output` only appears once it is complete.
pub fn strip_file(input: &Path, output: &Path) -> Result<Report, String> {
	let partial = with_suffix(output, ".partial");
	let unzipped = with_suffix(output, ".input.sql");

	let result = (|| {
		let (input, format) = match Format::sniff(input)? {
			Format::Zip => {
				unzip(input, &unzipped)?;
				(unzipped.as_path(), Format::Sql)
			}
			format => (input, format),
		};
		check_unpacked(input, format)?;
		let report = write(input, format, &partial, output)?;
		fs::rename(&partial, output).map_err(|e| {
			format!("Failed to write {}: {e}", output.display())
		})?;
		Ok(report)
	})();

	let _ = fs::remove_file(&unzipped);
	if result.is_err() {
		let _ = fs::remove_file(&partial);
	}
	result
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
	let mut path = path.as_os_str().to_owned();
	path.push(suffix);
	path.into()
}

fn open(input: &Path, format: Format) -> io::Result<Box<dyn BufRead>> {
	let file = File::open(input)?;
	Ok(match format {
		Format::Gzip => {
			Box::new(BufReader::new(GzDecoder::new(BufReader::new(file))))
		}
		_ => Box::new(BufReader::with_capacity(1 << 20, file)),
	})
}

/// Refuses an archive inside the archive: it would pass through unchanged.
fn check_unpacked(input: &Path, format: Format) -> Result<(), String> {
	let mut reader = open(input, format)
		.map_err(|e| format!("Failed to open {}: {e}", input.display()))?;
	let start = reader
		.fill_buf()
		.map_err(|e| format!("Failed to read {}: {e}", input.display()))?;

	match Format::of_bytes(start) {
		Format::Sql => Ok(()),
		_ => Err(format!(
			"{} holds another archive, unpack it first",
			input.display()
		)),
	}
}

/// Extracts the one file of a zip.
fn unzip(input: &Path, to: &Path) -> Result<(), String> {
	let error = |e: &dyn std::fmt::Display| {
		format!("Failed to unzip {}: {e}", input.display())
	};

	let file = File::open(input).map_err(|e| error(&e))?;
	let mut archive = ZipArchive::new(file).map_err(|e| error(&e))?;
	let mut sql = None;
	for i in 0..archive.len() {
		let entry = archive.by_index(i).map_err(|e| error(&e))?;
		if entry.is_dir() {
			continue;
		}
		if sql.is_some() {
			return Err(error(&"it has more than one file"));
		}
		sql = Some(i);
	}
	let sql = sql.ok_or_else(|| error(&"it is empty"))?;

	let mut entry = archive.by_index(sql).map_err(|e| error(&e))?;
	let mut out = BufWriter::new(File::create(to).map_err(|e| error(&e))?);
	io::copy(&mut entry, &mut out).map_err(|e| error(&e))?;
	out.flush().map_err(|e| error(&e))
}

fn write(
	input: &Path,
	format: Format,
	partial: &Path,
	output: &Path,
) -> Result<Report, String> {
	let error = |e: &dyn std::fmt::Display| {
		format!("Failed to write {}: {e}", output.display())
	};
	let file = BufWriter::with_capacity(
		1 << 20,
		File::create(partial).map_err(|e| error(&e))?,
	);

	match Format::of(output) {
		Format::Sql => {
			let mut out = file;
			let report = strip(|| open(input, format), &mut out)?;
			out.flush().map_err(|e| error(&e))?;
			Ok(report)
		}
		Format::Gzip => {
			let mut out = GzEncoder::new(file, Compression::default());
			let report = strip(|| open(input, format), &mut out)?;
			out.finish()
				.and_then(|mut file| file.flush())
				.map_err(|e| error(&e))?;
			Ok(report)
		}
		Format::Zip => {
			// the dump inside is named after the zip: berncity.sql.zip has
			// berncity.sql
			let name = output
				.file_name()
				.and_then(|name| name.to_str())
				.and_then(|name| name.strip_suffix(".zip"))
				.filter(|name| name.ends_with(".sql"))
				.unwrap_or("database.sql");
			let options = SimpleFileOptions::default()
				.compression_method(CompressionMethod::Deflated)
				.large_file(true);

			let mut out = ZipWriter::new(file);
			out.start_file(name, options).map_err(|e| error(&e))?;
			let report = strip(|| open(input, format), &mut out)?;
			out.finish()
				.map_err(|e| error(&e))?
				.flush()
				.map_err(|e| error(&e))?;
			Ok(report)
		}
	}
}
