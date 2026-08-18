use std::convert::TryFrom;
use std::{
    collections::HashMap,
    io::Read,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use ctap_crypto::Hash256;

use crate::totp::TotpAlgorithm;
use crate::vault_api::VAULT_PASSWORD_DICT;
use crate::vault_api::VAULT_TOTP_DICT;
use crate::vault_api::VAULT_BOOKMARKS_COUNTER_KEY;
use crate::vault_api::VAULT_BOOKMARKS_DICT;
const VAULT_TOTP_ALLOC_HINT: usize = 128;
const VAULT_PASSWORD_REC_VERSION: u32 = 1;

// Version history TOTP record:
//  - v1 created, basic record for TOTP
//  - v2 add HOTP support:
//    - `hotp` field added. If 1, then HOTP record. If not existent or not 1, then TOTP
//    - If HOTP, then the `timestep` field is re-purposed as the `count` field.
//    - v1 records read directly onto v2 records, and `hotp` is always `false` for v1 records
const VAULT_TOTP_REC_VERSION: u32 = 2;

#[derive(Debug)]
#[allow(dead_code)]
pub enum Error {
    IoError(std::io::Error),
    TotpSerError(TOTPSerializationError),
    PasswordSerError(PasswordSerializationError),
    KeyExists,
    DupesExist(Vec<usize>),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Self::IoError(e) }
}

impl From<TOTPSerializationError> for Error {
    fn from(e: TOTPSerializationError) -> Self { Self::TotpSerError(e) }
}

impl From<PasswordSerializationError> for Error {
    fn from(e: PasswordSerializationError) -> Self { Self::PasswordSerError(e) }
}

/// Errors returned by the bookmark storage methods.
#[derive(Debug)]
#[allow(dead_code)]
pub enum BookmarkError {
    IoError(std::io::Error),
    ParseError(String),
    CounterCorrupt,
}
impl From<std::io::Error> for BookmarkError {
    fn from(e: std::io::Error) -> Self { Self::IoError(e) }
}
pub struct Bookmark {
    pub key: String,
    pub url: String,
    pub label: String,
    pub timestamp_unix: u64,
}
const fn const_str_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() { return false; }
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut i = 0;
    while i < a_bytes.len() {
        if a_bytes[i] != b_bytes[i] { return false; }
        i += 1;
    }
    true
}
// Safety invariant: VAULT_BOOKMARKS_DICT must never be the dc34 dict.
// No new code path in storage.rs may open, read, write, or reference "dc34" (the gene dict).
// The string literal "dc34" may appear only in this comment and in the assertion message below.
const _: () = assert!(
    !const_str_eq(crate::vault_api::VAULT_BOOKMARKS_DICT, "dc34"),
    "VAULT_BOOKMARKS_DICT must never equal the gene dict"
);
pub struct Manager {
    pddb: pddb::Pddb,
}

pub trait StorageContent {
    fn settings(&self) -> ContentPDDBSettings;

    fn set_ctime(&mut self, value: u64);

    fn from_vec(&mut self, data: Vec<u8>) -> Result<(), Error>;
    fn to_vec(&self) -> Vec<u8>;

    fn hash(&self) -> Vec<u8>;
}

#[derive(Clone)]
pub struct ContentPDDBSettings {
    dict: String,
    alloc_hint: Option<usize>,
}

pub enum ContentKind {
    TOTP,
    Password,
}

impl ContentKind {
    fn settings(&self) -> ContentPDDBSettings {
        match self {
            ContentKind::TOTP => TotpRecord::default().settings(),
            ContentKind::Password => PasswordRecord::default().settings(),
        }
    }
}

impl Manager {
    pub fn new(_xns: &xous_names::XousNames) -> Manager { Manager { pddb: pddb::Pddb::new() } }

    fn pddb_exists(&self, dict: &str, key_name: &str, basis: Option<String>) -> bool {
        match self.pddb.get(
            dict,
            &key_name,
            basis.as_deref(),
            false,
            false,
            None,
            Some(dc34_vault::basis_change),
        ) {
            Ok(_) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => {
                log::error!("error while trying to lookup if a key exists, {}", e);
                false
            }
        }
    }

    pub(crate) fn pddb_store(
        &self,
        payload: &[u8],
        dict: &str,
        key_name: &str,
        alloc_hint: Option<usize>,
        basis: Option<String>,
        sync: bool,
        overwrite: bool,
    ) -> Result<(), Error> {
        if !overwrite && self.pddb_exists(dict, key_name, basis.clone()) {
            return Err(Error::KeyExists);
        }

        match self.pddb.get(
            dict,
            &key_name,
            basis.as_deref(),
            true, // if overwrite, we wanna create both the dict and the key if they don't
            // exist
            true,
            alloc_hint,
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut data) => match data.write(payload) {
                Ok(_) => match sync {
                    true => Ok(self.pddb.sync().unwrap_or(())),
                    false => Ok(()),
                },
                Err(e) => Err(Error::IoError(e)),
            },
            Err(e) => Err(Error::IoError(e)),
        }
    }

    pub(crate) fn pddb_get(&self, dict: &str, key_name: &str) -> Result<Vec<u8>, Error> {
        match self.pddb.get(dict, key_name, None, false, false, None, Some(dc34_vault::basis_change)) {
            Ok(mut record) => {
                let mut data = Vec::<u8>::new();
                record.read_to_end(&mut data)?;
                Ok(data)
            }
            Err(e) => return Err(Error::IoError(e)),
        }
    }

    fn basis_for_key(&self, dict: &str, key_name: &str) -> Result<String, Error> {
        match self.pddb.get(dict, key_name, None, false, false, None, Some(dc34_vault::basis_change)) {
            Ok(record) => Ok(record.attributes().expect("couldn't get key attributes").basis),
            Err(e) => return Err(Error::IoError(e)),
        }
    }

    pub fn new_record(
        &mut self,
        record: &mut dyn StorageContent,
        basis: Option<String>,
        overwrite: bool,
    ) -> Result<(), Error> {
        let record = record;
        let settings = record.settings();
        record.set_ctime(utc_now().timestamp() as u64);
        let serialized_record: Vec<u8> = record.to_vec();

        self.pddb_store(
            &serialized_record,
            &settings.dict,
            &hex(record.hash()),
            settings.alloc_hint,
            basis.clone(),
            true,
            overwrite,
        )
    }

    pub fn new_records(
        &mut self,
        records: Vec<Box<dyn StorageContent>>,
        basis: Option<String>,
        overwrite: bool,
    ) -> Result<(), Error> {
        let mut precords = records.into_iter().peekable();
        let mut current_idx = 0; // idk how to use peekable + enumerate
        let mut dupes = vec![];
        while let Some(record) = precords.next() {
            let mut record = record;
            let settings = record.settings();
            record.set_ctime(utc_now().timestamp() as u64);
            let serialized_record: Vec<u8> = record.to_vec();
            let should_sync = precords.peek().is_none() || current_idx % 10 == 0;

            log::debug!(
                "current_idx: {}, should_sync: {}, is_none: {}",
                current_idx,
                should_sync,
                precords.peek().is_none()
            );

            match self.pddb_store(
                &serialized_record,
                &settings.dict,
                &hex(record.hash()),
                settings.alloc_hint,
                basis.clone(),
                should_sync,
                overwrite,
            ) {
                Ok(()) => (),
                Err(error) => match error {
                    Error::KeyExists => {
                        dupes.push(current_idx);
                        ()
                    }
                    _ => return Err(error),
                },
            }

            current_idx += 1;
        }

        if !dupes.is_empty() {
            return Err(Error::DupesExist(dupes));
        }

        Ok(())
    }

    pub fn all<T: StorageContent + std::default::Default>(&self, kind: ContentKind) -> Result<Vec<T>, Error> {
        let settings = kind.settings();

        let keylist = self.pddb.list_keys(&settings.dict, None)?;

        let mut ret = vec![];

        for key in keylist {
            let mut record = T::default();
            record.from_vec(self.pddb_get(&settings.dict, &key)?)?;
            ret.push(record);
        }

        Ok(ret)
    }

    pub fn get_record<T: StorageContent + std::default::Default>(
        &self,
        kind: &ContentKind,
        key_name: &str,
    ) -> Result<T, Error> {
        let settings = kind.settings();
        let mut record = T::default();
        record.from_vec(self.pddb_get(&settings.dict, &key_name)?)?;

        Ok(record)
    }

    pub fn update(
        &mut self,
        kind: &ContentKind,
        key_name: &str,
        record: &mut dyn StorageContent,
    ) -> Result<(), Error> {
        let settings = kind.settings();

        let basis = self.basis_for_key(&settings.dict, key_name)?;
        self.pddb.delete_key(&settings.dict, key_name, Some(&basis))?;

        self.new_record(&mut *record, Some(basis), true)
    }

    pub fn delete(&mut self, kind: ContentKind, key_name: &str) -> Result<(), Error> {
        let settings = kind.settings();

        let basis = self.basis_for_key(&settings.dict, key_name)?;
        self.pddb.delete_key(&settings.dict, key_name, Some(&basis)).map_err(|error| Error::IoError(error))
    }

    pub(crate) fn bookmark_next_key(&mut self) -> Result<String, BookmarkError> {
        let current: u64 = match self.pddb.get(
            VAULT_BOOKMARKS_DICT,
            VAULT_BOOKMARKS_COUNTER_KEY,
            None,
            false,
            false,
            None,
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut record) => {
                let mut data = Vec::new();
                record.read_to_end(&mut data)?;
                let s = std::str::from_utf8(&data)
                    .map_err(|_| BookmarkError::CounterCorrupt)?;
                u64::from_str_radix(s.trim(), 16)
                    .map_err(|_| BookmarkError::CounterCorrupt)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0u64,
            Err(e) => return Err(BookmarkError::IoError(e)),
        };
        let next = current + 1;
        let key = format!("{:016x}", next);
        let counter_bytes = format!("{:x}", next).into_bytes();
        match self.pddb.get(
            VAULT_BOOKMARKS_DICT,
            VAULT_BOOKMARKS_COUNTER_KEY,
            None,
            true,
            true,
            Some(20),
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut record) => { record.write_all(&counter_bytes)?; }
            Err(e) => return Err(BookmarkError::IoError(e)),
        }
        Ok(key)
    }

    pub(crate) fn bookmark_store(&mut self, url: &str, label: &str) -> Result<String, BookmarkError> {
        let key = self.bookmark_next_key()?;
        let timestamp_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let body = format!("{}\n{}\n{}", url, label, timestamp_unix);
        let body_bytes = body.into_bytes();
        match self.pddb.get(
            VAULT_BOOKMARKS_DICT,
            &key,
            None,
            true,
            true,
            Some(VAULT_TOTP_ALLOC_HINT),
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut record) => { record.write_all(&body_bytes)?; }
            Err(e) => return Err(BookmarkError::IoError(e)),
        }
        self.pddb.sync().unwrap_or(());
        Ok(key)
    }

    pub(crate) fn bookmark_get(&mut self, key: &str) -> Result<Bookmark, BookmarkError> {
        match self.pddb.get(
            VAULT_BOOKMARKS_DICT,
            key,
            None,
            false,
            false,
            None,
            Some(dc34_vault::basis_change),
        ) {
            Ok(mut record) => {
                let mut data = Vec::new();
                record.read_to_end(&mut data)?;
                Self::parse_bookmark_body(key, &data)
            }
            Err(e) => Err(BookmarkError::IoError(e)),
        }
    }

    fn parse_bookmark_body(key: &str, data: &[u8]) -> Result<Bookmark, BookmarkError> {
        let body = std::str::from_utf8(data)
            .map_err(|_| BookmarkError::ParseError("non-UTF8 body".into()))?;
        let mut lines = body.splitn(3, '\n');
        let url = lines
            .next()
            .ok_or_else(|| BookmarkError::ParseError("missing url field".into()))?
            .to_string();
        let label = lines
            .next()
            .ok_or_else(|| BookmarkError::ParseError("missing label field".into()))?
            .to_string();
        let ts_str = lines
            .next()
            .ok_or_else(|| BookmarkError::ParseError("missing timestamp field".into()))?;
        let timestamp_unix = ts_str
            .trim()
            .parse::<u64>()
            .map_err(|_| BookmarkError::ParseError(format!("bad timestamp: {:?}", ts_str)))?;
        Ok(Bookmark { key: key.to_string(), url, label, timestamp_unix })
    }
}

#[derive(Default)]
pub struct TotpRecord {
    pub version: u32,
    // as base32, RFC4648 no padding
    pub secret: String,
    pub name: String,
    pub algorithm: TotpAlgorithm,
    pub notes: String,
    pub digits: u32,
    pub timestep: u64,
    pub ctime: u64,
    pub is_hotp: bool,
}

#[derive(Debug)]
pub enum TOTPSerializationError {
    BadVersion,
    BadAlgorithm,
    BadDigitsAmount,
    BadCtime,
    BadTimestep,
    BadHotp,
    MalformedInput,
}

impl TotpRecord {
    pub fn from_uri(uri: &str) -> Result<TotpRecord, String> {
        let mut record = TotpRecord::default();

        // Split on first '/' to get type and rest
        let (otp_type, rest) = uri.split_once('/').ok_or("Invalid format: missing '/'")?;

        // Determine if HOTP or TOTP
        record.is_hotp = match otp_type.to_lowercase().as_str() {
            "hotp" => true,
            "totp" => false,
            _ => return Err(format!("Unknown OTP type: {}", otp_type)),
        };

        // Split label and query
        let (label, query) = rest.split_once('?').ok_or("Missing query parameters")?;

        // Parse label (may be "issuer:account" or just "account")
        let decoded_label = url_decode(label)?;
        /*
        record.name = if let Some((_, account)) = decoded_label.split_once(':') {
            account.to_string()
        } else {
            decoded_label
        };
        */
        // don't parse this field - we want the issuer to be shown.
        record.name = decoded_label;

        // Parse query parameters
        let params = parse_query(query)?;

        // Extract secret (required)
        record.secret = params.get("secret").ok_or("Missing required 'secret' parameter")?.to_string();

        // Extract optional parameters with defaults
        if let Some(algo) = params.get("algorithm") {
            record.algorithm = match algo.to_uppercase().as_str() {
                "SHA1" => TotpAlgorithm::HmacSha1,
                "SHA256" => TotpAlgorithm::HmacSha256,
                "SHA512" => TotpAlgorithm::HmacSha512,
                _ => return Err(format!("Unknown algorithm: {}", algo)),
            };
        }

        if let Some(digits) = params.get("digits") {
            record.digits = digits.parse().map_err(|_| format!("Invalid digits value: {}", digits))?;
        } else {
            record.digits = 6; // Default
        }

        if !record.is_hotp {
            if let Some(period) = params.get("period") {
                record.timestep = period.parse().map_err(|_| format!("Invalid period value: {}", period))?;
            } else {
                record.timestep = 30; // Default
            }
        }

        // Extract issuer for notes if present
        if let Some(issuer) = params.get("issuer") {
            record.notes = issuer.clone();
        }

        Ok(record)
    }
}

fn parse_query(query: &str) -> Result<HashMap<String, String>, String> {
    let mut params = HashMap::new();

    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            let decoded_value = url_decode(value)?;
            params.insert(key.to_string(), decoded_value);
        }
    }

    Ok(params)
}

fn url_decode(s: &str) -> Result<String, String> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '%' => {
                // Get next two characters as hex
                let hex: String = chars.by_ref().take(2).collect();
                if hex.len() != 2 {
                    return Err("Invalid percent encoding".to_string());
                }

                let byte = u8::from_str_radix(&hex, 16)
                    .map_err(|_| format!("Invalid hex in percent encoding: {}", hex))?;

                result.push(byte as char);
            }
            '+' => result.push(' '),
            _ => result.push(ch),
        }
    }

    Ok(result)
}

impl StorageContent for TotpRecord {
    fn settings(&self) -> ContentPDDBSettings {
        ContentPDDBSettings { dict: VAULT_TOTP_DICT.to_string(), alloc_hint: Some(VAULT_TOTP_ALLOC_HINT) }
    }

    fn set_ctime(&mut self, value: u64) { self.ctime = value; }

    fn from_vec(&mut self, data: Vec<u8>) -> Result<(), Error> {
        let desc_str = std::str::from_utf8(&data).or(Err(TOTPSerializationError::MalformedInput))?;

        let mut pr = TotpRecord::default();

        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(TOTPSerializationError::BadVersion)?;
                        }
                    }
                    "secret" => pr.secret.push_str(data),
                    "name" => pr.name.push_str(data),
                    "algorithm" => {
                        pr.algorithm = match TotpAlgorithm::try_from(data) {
                            Ok(a) => a,
                            Err(_) => return Err(TOTPSerializationError::BadAlgorithm)?,
                        }
                    }
                    "notes" => pr.notes.push_str(data),
                    "digits" => {
                        if let Ok(digits) = u32::from_str_radix(data, 10) {
                            pr.digits = digits;
                        } else {
                            log::warn!("digits error");
                            return Err(TOTPSerializationError::BadDigitsAmount)?;
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(TOTPSerializationError::BadCtime)?;
                        }
                    }
                    "timestep" => {
                        if let Ok(timestep) = u64::from_str_radix(data, 10) {
                            pr.timestep = timestep;
                        } else {
                            log::warn!("timestep error");
                            return Err(TOTPSerializationError::BadTimestep)?;
                        }
                    }
                    "hotp" => {
                        if let Ok(setting) = u8::from_str_radix(data, 10) {
                            if setting != 0 {
                                pr.is_hotp = true;
                            } else {
                                pr.is_hotp = false;
                            }
                        } else {
                            log::warn!("hotp variant error");
                            return Err(TOTPSerializationError::BadHotp)?;
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing TOTP info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }

        *self = pr;

        Ok(())
    }

    fn to_vec(&self) -> Vec<u8> {
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            self.version,
            "secret",
            self.secret,
            "name",
            self.name,
            "algorithm",
            self.algorithm,
            "notes",
            self.notes,
            "digits",
            self.digits,
            "timestep",
            self.timestep,
            "hotp",
            if self.is_hotp { 1 } else { 0 },
            "ctime",
            self.ctime,
        )
        .into_bytes()
    }

    fn hash(&self) -> Vec<u8> {
        let mut h = ctap_crypto::sha256::Sha256::new();
        h.update(self.name.as_bytes());
        h.finalize().to_vec()
    }
}

impl TryFrom<Vec<u8>> for TotpRecord {
    type Error = TOTPSerializationError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        log::info!("{:?}", value);
        let desc_str = std::str::from_utf8(&value).or(Err(TOTPSerializationError::MalformedInput))?;
        log::info!("{:?}", desc_str);

        let mut pr = TotpRecord {
            version: VAULT_TOTP_REC_VERSION,
            secret: String::new(),
            name: String::new(),
            algorithm: TotpAlgorithm::HmacSha1,
            notes: String::new(),
            digits: 0,
            ctime: 0,
            timestep: 0,
            is_hotp: false,
        };
        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(TOTPSerializationError::BadVersion);
                        }
                    }
                    "secret" => pr.secret.push_str(data),
                    "name" => pr.name.push_str(data),
                    "algorithm" => {
                        pr.algorithm = match TotpAlgorithm::try_from(data) {
                            Ok(a) => a,
                            Err(_) => return Err(TOTPSerializationError::BadAlgorithm),
                        }
                    }
                    "notes" => pr.notes.push_str(data),
                    "digits" => {
                        if let Ok(digits) = u32::from_str_radix(data, 10) {
                            pr.digits = digits;
                        } else {
                            log::warn!("digits error");
                            return Err(TOTPSerializationError::BadDigitsAmount);
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(TOTPSerializationError::BadCtime);
                        }
                    }
                    "timestep" => {
                        if let Ok(timestep) = u64::from_str_radix(data, 10) {
                            pr.timestep = timestep;
                        } else {
                            log::warn!("timestep error");
                            return Err(TOTPSerializationError::BadTimestep);
                        }
                    }
                    "hotp" => {
                        if let Ok(setting) = u8::from_str_radix(data, 10) {
                            if setting != 0 {
                                pr.is_hotp = true;
                            } else {
                                pr.is_hotp = false;
                            }
                        } else {
                            log::warn!("hotp error");
                            return Err(TOTPSerializationError::BadHotp);
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing TOTP info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }

        Ok(pr)
    }
}

impl From<TotpRecord> for Vec<u8> {
    fn from(tr: TotpRecord) -> Self {
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            tr.version,
            "secret",
            tr.secret,
            "name",
            tr.name,
            "algorithm",
            tr.algorithm,
            "notes",
            tr.notes,
            "digits",
            tr.digits,
            "timestep",
            tr.timestep,
            "hotp",
            if tr.is_hotp { 1 } else { 0 },
            "ctime",
            tr.ctime,
        )
        .into_bytes()
    }
}

#[derive(Debug)]
pub enum PasswordSerializationError {
    MalformedInput,
    BadVersion,
    BadCount,
    BadCtime,
    BadAtime,
}

#[derive(Default)]
pub struct PasswordRecord {
    pub version: u32,
    // This contains the URL
    pub description: String,
    // Username associated with the URL
    pub username: String,
    pub password: String,
    // Initially contains just "Notes", but will contain the old password when updated
    pub notes: String,
    pub ctime: u64,
    pub atime: u64,
    pub count: u64,
}
impl PasswordRecord {
    pub fn alloc() -> Self {
        // The intent is only one of these is allocated, and it is re-used.
        // Sizes picked to be big enough to probably avoid re-allocs,
        // yet small enough to not be unreasonable for a temporary buffer.
        PasswordRecord {
            version: 0,
            description: String::with_capacity(256),
            username: String::with_capacity(256),
            password: String::with_capacity(256),
            notes: String::with_capacity(1024),
            ctime: 0,
            atime: 0,
            count: 0,
        }
    }

    pub fn clear(&mut self) {
        self.description.clear();
        self.username.clear();
        self.password.clear();
        self.notes.clear();
        self.version = 0;
        self.ctime = 0;
        self.atime = 0;
        self.count = 0;
    }
}

impl StorageContent for PasswordRecord {
    fn settings(&self) -> ContentPDDBSettings {
        ContentPDDBSettings { dict: VAULT_PASSWORD_DICT.to_string(), alloc_hint: Some(VAULT_TOTP_ALLOC_HINT) }
    }

    fn set_ctime(&mut self, value: u64) { self.ctime = value; }

    fn from_vec(&mut self, data: Vec<u8>) -> Result<(), Error> {
        self.clear();
        // use `std::str` so we're allocating this temporary on the stack
        let desc_str = std::str::from_utf8(&data).or(Err(PasswordSerializationError::MalformedInput))?;

        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            self.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(PasswordSerializationError::BadVersion)?;
                        }
                    }
                    "description" => self.description.push_str(data),
                    "username" => self.username.push_str(data),
                    "password" => self.password.push_str(data),
                    "notes" => self.notes.push_str(data),
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            self.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(PasswordSerializationError::BadCtime)?;
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            self.atime = atime;
                        } else {
                            log::warn!("atime error");
                            return Err(PasswordSerializationError::BadAtime)?;
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            self.count = count;
                        } else {
                            log::warn!("count error");
                            return Err(PasswordSerializationError::BadCount)?;
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing password info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        Ok(())
    }

    fn to_vec(&self) -> Vec<u8> {
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            self.version,
            "description",
            self.description,
            "username",
            self.username,
            "password",
            self.password,
            "notes",
            self.notes,
            "ctime",
            self.ctime,
            "atime",
            self.atime,
            "count",
            self.count,
        )
        .into_bytes()
    }

    fn hash(&self) -> Vec<u8> {
        let mut h = ctap_crypto::sha256::Sha256::new();
        h.update(self.description.as_bytes());
        h.update(self.username.as_bytes());
        h.finalize().to_vec()
    }
}

impl TryFrom<Vec<u8>> for PasswordRecord {
    type Error = PasswordSerializationError;

    fn try_from(data: Vec<u8>) -> Result<Self, Self::Error> {
        let desc_str = String::from_utf8(data).or(Err(PasswordSerializationError::MalformedInput))?;

        let mut pr = PasswordRecord {
            version: VAULT_PASSWORD_REC_VERSION,
            description: String::new(),
            username: String::new(),
            password: String::new(),
            notes: String::new(),
            ctime: 0,
            atime: 0,
            count: 0,
        };

        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            log::warn!("ver error");
                            return Err(PasswordSerializationError::BadVersion);
                        }
                    }
                    "description" => pr.description.push_str(data),
                    "username" => pr.username.push_str(data),
                    "password" => pr.password.push_str(data),
                    "notes" => pr.notes.push_str(data),
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return Err(PasswordSerializationError::BadCtime);
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            pr.atime = atime;
                        } else {
                            log::warn!("atime error");
                            return Err(PasswordSerializationError::BadAtime);
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            pr.count = count;
                        } else {
                            log::warn!("count error");
                            return Err(PasswordSerializationError::BadCount);
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing password info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        Ok(pr)
    }
}

impl From<PasswordRecord> for Vec<u8> {
    fn from(pr: PasswordRecord) -> Self {
        format!(
            "{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
            "version",
            pr.version,
            "description",
            pr.description,
            "username",
            pr.username,
            "password",
            pr.password,
            "notes",
            pr.notes,
            "ctime",
            pr.ctime,
            "atime",
            pr.atime,
            "count",
            pr.count,
        )
        .into_bytes()
    }
}

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid
/// target
fn utc_now() -> DateTime<Utc> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
    DateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32).unwrap()
}

pub fn hex(data: Vec<u8>) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(2 * data.len());
    for byte in data {
        write!(s, "{:02X}", byte).unwrap();
    }

    s
}

#[cfg(test)]
mod bookmark_tests {
    use super::*;

    #[test]
    fn test_bookmark_key_format() {
        assert_eq!(format!("{:016x}", 1u64), "0000000000000001");
        assert_eq!(format!("{:016x}", 100u64), "0000000000000064");
        assert_eq!(format!("{:016x}", u64::MAX), "ffffffffffffffff");
    }

    #[test]
    fn test_bookmark_key_sort_order() {
        let mut keys = vec![
            format!("{:016x}", 3u64),
            format!("{:016x}", 1u64),
            format!("{:016x}", 2u64),
        ];
        keys.sort();
        assert_eq!(keys[0], format!("{:016x}", 1u64));
        assert_eq!(keys[1], format!("{:016x}", 2u64));
        assert_eq!(keys[2], format!("{:016x}", 3u64));
    }

    #[test]
    fn test_bookmark_body_roundtrip() {
        let url = "https://example.com/path?q=1";
        let label = "Example";
        let timestamp: u64 = 1_700_000_000;
        let body = format!("{}\n{}\n{}", url, label, timestamp);
        let bm = Manager::parse_bookmark_body("0000000000000001", body.as_bytes()).unwrap();
        assert_eq!(bm.key, "0000000000000001");
        assert_eq!(bm.url, url);
        assert_eq!(bm.label, label);
        assert_eq!(bm.timestamp_unix, timestamp);
    }

    #[test]
    fn test_bookmark_body_missing_fields() {
        let body = b"https://example.com";
        let result = Manager::parse_bookmark_body("0000000000000001", body);
        assert!(result.is_err(), "body missing label and timestamp should be rejected");
    }

    #[test]
    fn test_counter_key_filtered() {
        let keys = vec![
            "0000000000000001".to_string(),
            "__counter__".to_string(),
            "0000000000000002".to_string(),
        ];
        let filtered: Vec<_> = keys.iter().filter(|k| k.as_str() != "__counter__").collect();
        assert_eq!(filtered.len(), 2);
        assert!(!filtered.iter().any(|k| k.as_str() == "__counter__"));
    }

    #[test]
    fn test_bookmarks_dict_is_not_dc34() {
        assert_ne!(
            crate::vault_api::VAULT_BOOKMARKS_DICT,
            "dc34",
            "VAULT_BOOKMARKS_DICT must never equal the gene dict"
        );
    }

    #[test]
    fn test_bookmark_body_non_utf8() {
        let bad: &[u8] = &[0xFF, 0xFE, b'\n', b'l', b'\n', b'0'];
        let result = Manager::parse_bookmark_body("key", bad);
        assert!(matches!(result, Err(BookmarkError::ParseError(_))));
    }
}


// ---- S-CAM settings: default bookmark ----
pub(crate) const VAULT_SETTINGS_DICT: &str = "vault.settings";
// ---- PASSKEYS: FIDO2 / U2F credential listing ----
//
// These credentials were stored but had no screen: the badge acted as a security key without
// ever showing what it held. That invisible third credential type is what made the old menu
// confusing, since passwords and TOTP each had a screen and this did not.
//
// Records live in `fido.u2fapps`, keyed by hex app id, with a serialised AppInfo body.


/// One entry for the PASSKEYS screen.
pub(crate) struct Passkey {
    /// PDDB key - the hex app id. Needed to delete the record.
    pub key: String,
    /// Human-readable site name, falling back to a short form of the id.
    pub name: String,
}

pub(crate) fn passkey_list(pddb: &pddb::Pddb) -> Vec<Passkey> {
    let keys = match pddb.list_keys(crate::vault_api::U2F_APP_DICT, None) {
        Ok(k) => k,
        Err(e) => {
            log::warn!("passkey_list: list_keys failed: {:?}", e);
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for key in keys {
        let name = read_passkey_name(pddb, &key)
            // an unnamed credential still deserves a stable label rather than a blank row
            // chars(), not a byte slice. `key.len().min(8)` bounds the length but not the
            // UTF-8 boundary, so a key whose 8th byte lands mid-character panics - and this
            // process is the whole UI. Passkey keys are hex app ids in practice, but they come
            // from whatever is in the dictionary, which is exactly the assumption the upstream
            // QR fix was about.
            .unwrap_or_else(|| format!("({}…)", key.chars().take(8).collect::<String>()));
        out.push(Passkey { key, name });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn read_passkey_name(pddb: &pddb::Pddb, key: &str) -> Option<String> {
    let mut k = pddb
        .get(crate::vault_api::U2F_APP_DICT, key, None, false, false, None, None::<fn()>)
        .ok()?;
    let mut buf = Vec::<u8>::new();
    use std::io::Read;
    k.read_to_end(&mut buf).ok()?;
    let info = crate::vault_api::deserialize_app_info(buf)?;
    if info.name.trim().is_empty() { None } else { Some(info.name) }
}

// ---- PHOTOS and standby images ----
//
// A captured photo is the 128x128 thresholded framebuffer, 2048 bytes, bit-identical to the
// badge's compiled-in bitmaps. That means a photo can be rendered through the existing bitmap
// path and can be chosen as the standby image without any conversion.

// The cap and the photo geometry are shared with dc34-console through dc34-api, so the
// badge and the REPL cannot disagree about how many photos fit.
pub(crate) use dc34_api::{PHOTO_BYTES, PHOTO_CAP, VAULT_PHOTOS_DICT};
pub(crate) const SETTING_STANDBY: &str = "standby_image";
pub(crate) const SETTING_BLINKY: &str = "blinky_pattern";

pub(crate) fn photo_list(pddb: &pddb::Pddb) -> Vec<String> {
    let mut keys = pddb.list_keys(VAULT_PHOTOS_DICT, None).unwrap_or_default();
    keys.sort();
    keys
}

/// Store a capture. Returns None when the cap is reached, so the caller can tell the user
/// rather than silently discarding the shot.
pub(crate) fn photo_store(pddb: &pddb::Pddb, data: &[u32; 512]) -> Option<String> {
    let existing = photo_list(pddb);
    if existing.len() >= PHOTO_CAP {
        log::info!("photo cap of {} reached", PHOTO_CAP);
        return None;
    }
    // monotonic key so ordering is stable and a delete cannot cause a collision
    let next = existing
        .iter()
        .filter_map(|k| k.strip_prefix("photo_").and_then(|n| n.parse::<u32>().ok()))
        .max()
        .map(|n| n + 1)
        .unwrap_or(0);
    let key = format!("photo_{:04}", next);
    let bytes: &[u8] = bytemuck::cast_slice(data);
    let mut k = pddb
        .get(VAULT_PHOTOS_DICT, &key, None, true, true, Some(PHOTO_BYTES), None::<fn()>)
        .ok()?;
    use std::io::Write;
    k.write_all(bytes).ok()?;
    Some(key)
}

pub(crate) fn photo_get(pddb: &pddb::Pddb, key: &str) -> Option<[u32; 512]> {
    let mut buf = [0u8; PHOTO_BYTES];
    let mut k = pddb
        .get(VAULT_PHOTOS_DICT, key, None, false, false, Some(PHOTO_BYTES), None::<fn()>)
        .ok()?;
    use std::io::Read;
    if k.read(&mut buf).ok()? != PHOTO_BYTES {
        return None;
    }
    let words: &[u32] = bytemuck::cast_slice(&buf);
    words.try_into().ok()
}

pub(crate) fn photo_delete(pddb: &pddb::Pddb, key: &str) -> Result<(), std::io::Error> {
    pddb.delete_key(VAULT_PHOTOS_DICT, key, None)
}

/// Free-function form for the UX side, which holds a Pddb rather than a Storage. The method
/// on Storage stays for the actions thread, which holds one.
pub(crate) fn bookmark_delete(pddb: &pddb::Pddb, key: &str) -> Result<(), std::io::Error> {
    pddb.delete_key(VAULT_BOOKMARKS_DICT, key, None)?;
    pddb.sync().unwrap_or(());
    Ok(())
}

fn set_usize(pddb: &pddb::Pddb, key: &str, v: usize) -> Result<(), std::io::Error> {
    let mut k = pddb.get(VAULT_SETTINGS_DICT, key, None, true, true, Some(8), None::<fn()>)?;
    use std::io::Write;
    k.write_all(&(v as u32).to_le_bytes())?;
    Ok(())
}

fn get_usize(pddb: &pddb::Pddb, key: &str) -> Option<usize> {
    let mut buf = [0u8; 4];
    let mut k = pddb.get(VAULT_SETTINGS_DICT, key, None, false, false, Some(8), None::<fn()>).ok()?;
    use std::io::Read;
    if k.read(&mut buf).ok()? != 4 { return None; }
    Some(u32::from_le_bytes(buf) as usize)
}

pub(crate) fn set_standby_choice(pddb: &pddb::Pddb, v: usize) -> Result<(), std::io::Error> {
    set_usize(pddb, SETTING_STANDBY, v)
}
pub(crate) fn standby_choice(pddb: &pddb::Pddb) -> usize {
    get_usize(pddb, SETTING_STANDBY).unwrap_or(0)
}
pub(crate) fn set_blinky_choice(pddb: &pddb::Pddb, v: usize) -> Result<(), std::io::Error> {
    set_usize(pddb, SETTING_BLINKY, v)
}
pub(crate) fn blinky_choice(pddb: &pddb::Pddb) -> usize {
    get_usize(pddb, SETTING_BLINKY).unwrap_or(0)
}
