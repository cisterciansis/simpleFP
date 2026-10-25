//! SimpleFP lab store: animals, cages, breeding, surgeries, sessions, cohorts,
//! exclusions and notes in one local SQLite file per lab.
//!
//! Every write goes through [`Lab`], which validates it and appends a
//! hash-chained [`audit`](Lab::verify_audit) row in the same transaction.
//! Rows cross this API as JSON objects whose keys are column names, so the
//! schema in `schema.sql` is the single definition of the data model.

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = include_str!("schema.sql");

/// Tables writable through the generic API. Junction and log tables are written
/// only by domain operations so their invariants cannot be bypassed.
const GENERIC: &[&str] = &[
    "user", "protocol", "line", "cage", "breeding_pair", "litter", "animal", "genotype_call", "virus_lot", "procedure",
    "histology", "session", "video", "cohort", "exclusion", "note",
];
/// Tables readable through the generic API.
const READABLE: &[&str] = &[
    "user", "protocol", "line", "cage", "breeding_pair", "breeding_female", "litter", "animal", "genotype_call",
    "cage_assignment", "virus_lot", "procedure", "surgery_frame", "injection", "implant", "histology", "session",
    "video", "cohort", "cohort_member", "cohort_session", "exclusion", "note", "audit",
];

pub type Result<T> = std::result::Result<T, String>;

fn db<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Days since 1970-01-01 for a proleptic Gregorian `YYYY-MM-DD` (Hinnant's algorithm).
pub fn days(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (date[..4].parse().ok()?, date[5..7].parse().ok()?, date[8..].parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d < 1 || d > dim[(m - 1) as usize] {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Inverse of [`days`].
pub fn date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn is_date_column(c: &str) -> bool {
    c.ends_with("_on") || c == "dob" || c == "death_date"
}

fn to_sql(col: &str, v: &Value) -> Result<Sql> {
    Ok(match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Sql::Integer(i),
            None => Sql::Real(n.as_f64().ok_or_else(|| format!("{col}: number out of range"))?),
        },
        Value::String(s) => {
            let s = s.trim();
            if s.len() > 20_000 {
                return Err(format!("{col}: value too long"));
            }
            if is_date_column(col) && days(s).is_none() {
                return Err(format!("{col}: '{s}' is not a date (YYYY-MM-DD)"));
            }
            Sql::Text(s.to_string())
        }
        _ => return Err(format!("{col}: nested values are not allowed; store JSON as a string")),
    })
}

fn to_json(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => json!(format!("<{} bytes>", b.len())),
    }
}

fn rows(tx: &Connection, sql: &str, args: &[Sql]) -> Result<Vec<Map<String, Value>>> {
    let mut st = tx.prepare(sql).map_err(db)?;
    let cols: Vec<String> = st.column_names().map(str::to_owned).collect();
    let out = st
        .query_map(params_from_iter(args), |r| {
            let mut m = Map::new();
            for (i, c) in cols.iter().enumerate() {
                m.insert(c.clone(), to_json(r.get_ref(i)?));
            }
            Ok(m)
        })
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)?;
    Ok(out)
}

fn columns(tx: &Connection, table: &str) -> Result<Vec<String>> {
    let mut st = tx.prepare(&format!("PRAGMA table_info({table})")).map_err(db)?;
    let v = st.query_map([], |r| r.get::<_, String>(1)).map_err(db)?.collect::<std::result::Result<Vec<_>, _>>().map_err(db)?;
    Ok(v)
}

fn audit(tx: &Transaction, actor: &str, entity: &str, id: &str, op: &str, diff: &Value, reason: Option<&str>) -> Result<()> {
    let prev: String = tx.query_row("SELECT hash FROM audit ORDER BY seq DESC LIMIT 1", [], |r| r.get(0)).optional().map_err(db)?.unwrap_or_else(|| "genesis".into());
    let at = now_ms();
    let diff = diff.to_string();
    let hash = chain_hash(&prev, at, actor, entity, id, op, &diff, reason);
    tx.execute(
        "INSERT INTO audit (at, actor, entity, entity_id, op, diff, reason, prev_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![at, actor, entity, id, op, diff, reason, prev, hash],
    )
    .map_err(db)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn chain_hash(prev: &str, at: i64, actor: &str, entity: &str, id: &str, op: &str, diff: &str, reason: Option<&str>) -> String {
    let mut h = blake3::Hasher::new();
    for part in [prev, &at.to_string(), actor, entity, id, op, diff, reason.unwrap_or("")] {
        h.update(&(part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    }
    h.finalize().to_hex().to_string()
}

fn insert_row(tx: &Transaction, actor: &str, table: &str, mut fields: Map<String, Value>) -> Result<String> {
    let cols = columns(tx, table)?;
    if cols.iter().any(|c| c == "id") && !fields.contains_key("id") {
        fields.insert("id".into(), json!(new_id()));
    }
    if cols.iter().any(|c| c == "created_at") && !fields.contains_key("created_at") {
        fields.insert("created_at".into(), json!(now_ms()));
    }
    let mut names = vec![];
    let mut vals = vec![];
    for (k, v) in &fields {
        if !cols.contains(k) {
            return Err(format!("{table} has no field '{k}'"));
        }
        names.push(k.clone());
        vals.push(to_sql(k, v)?);
    }
    let marks = (1..=names.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    tx.execute(&format!("INSERT INTO {table} ({}) VALUES ({marks})", names.join(", ")), params_from_iter(&vals)).map_err(|e| friendly(table, e))?;
    let id = fields.get("id").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| names.iter().map(|n| fields[n].to_string()).collect::<Vec<_>>().join("|"));
    audit(tx, actor, table, &id, "create", &Value::Object(fields), None)?;
    Ok(id)
}

/// Turn SQLite constraint errors into messages a person can act on.
fn friendly(table: &str, e: rusqlite::Error) -> String {
    let m = e.to_string();
    if m.contains("UNIQUE constraint failed: cage_assignment.animal_id") || m.contains("one_open_assignment") {
        "the animal is already in a cage; move it instead".into()
    } else if m.contains("UNIQUE constraint failed") {
        format!("{table}: {} already exists", m.rsplit(": ").next().unwrap_or("value"))
    } else if m.contains("FOREIGN KEY constraint failed") {
        format!("{table}: refers to a record that does not exist")
    } else if m.contains("CHECK constraint failed") {
        format!("{table}: a value is outside what is allowed ({m})")
    } else if m.contains("NOT NULL constraint failed") {
        format!("{table}: required field missing ({})", m.rsplit('.').next().unwrap_or(""))
    } else {
        format!("{table}: {m}")
    }
}

/// One pup to create at weaning.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pup {
    pub label: String,
    pub sex: String,
    #[serde(default)]
    pub ear_tag: Option<String>,
    pub cage_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surgery {
    /// `procedure` columns (kind defaults to stereotaxic_surgery).
    pub procedure: Map<String, Value>,
    pub frame: Map<String, Value>,
    #[serde(default)]
    pub injections: Vec<Map<String, Value>>,
    #[serde(default)]
    pub implants: Vec<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub due_on: String,
    pub kind: String,
    pub title: String,
    pub entity: String,
    pub entity_id: String,
    pub overdue: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineItem {
    /// `YYYY-MM-DD` or an ISO-ish instant; sortable as text.
    pub when: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub entity: String,
    pub entity_id: String,
}

pub struct Lab {
    conn: Connection,
    actor: String,
}

impl Lab {
    /// Open (creating if needed) a lab store. `actor` is recorded on every audit row.
    pub fn open(path: &Path, actor: &str) -> Result<Lab> {
        if actor.trim().is_empty() {
            return Err("an actor (user initials) is required".into());
        }
        let conn = Connection::open(path).map_err(db)?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;").map_err(db)?;
        let has_meta: bool = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'meta'", [], |r| r.get::<_, i64>(0)).map_err(db)? > 0;
        if !has_meta {
            conn.execute_batch(&format!("BEGIN; {SCHEMA} INSERT INTO meta VALUES ('schema', '{SCHEMA_VERSION}'); COMMIT;")).map_err(db)?;
        }
        let v: String = conn.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).map_err(db)?;
        if v != SCHEMA_VERSION.to_string() {
            return Err(format!("lab store schema {v} is not supported by this version (expects {SCHEMA_VERSION})"));
        }
        Ok(Lab { conn, actor: actor.trim().to_string() })
    }

    fn tx(&mut self) -> Result<(Transaction<'_>, String)> {
        let actor = self.actor.clone();
        Ok((self.conn.transaction().map_err(db)?, actor))
    }

    fn check_protocol(tx: &Connection, protocol_id: Option<&Value>, on: &str) -> Result<()> {
        let Some(pid) = protocol_id.and_then(Value::as_str) else { return Ok(()) };
        let row: Option<(String, String)> = tx.query_row("SELECT number, expires_on FROM protocol WHERE id = ?1 AND archived_at IS NULL", [pid], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(db)?;
        match row {
            None => Err("protocol not found or archived".into()),
            Some((n, exp)) if exp.as_str() < on => Err(format!("protocol {n} expired on {exp}; renew it before adding animals, cages or procedures")),
            _ => Ok(()),
        }
    }

    /// Create a row in a generic table. Returns its id.
    pub fn insert(&mut self, table: &str, fields: Map<String, Value>) -> Result<String> {
        if !GENERIC.contains(&table) {
            return Err(format!("'{table}' cannot be created directly"));
        }
        // Validate every value before any rule reads it (dates compare as text).
        for (k, v) in &fields {
            to_sql(k, v)?;
        }
        let (tx, actor) = self.tx()?;
        if ["animal", "cage", "procedure"].contains(&table) {
            let on = fields.get("performed_on").or(fields.get("opened_on")).and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| date(now_ms().div_euclid(86_400_000)));
            Self::check_protocol(&tx, fields.get("protocol_id"), &on)?;
        }
        if table == "cohort"
            && let Some(Value::String(ev)) = fields.get("events") {
                serde_json::from_str::<Vec<String>>(ev).map_err(|_| "cohort.events must be a JSON array of epoc names".to_string())?;
            }
        let id = insert_row(&tx, &actor, table, fields)?;
        tx.commit().map_err(db)?;
        Ok(id)
    }

    /// Change fields of an existing row. History-bearing fields (location, ids, timestamps) are not editable here.
    pub fn update(&mut self, table: &str, id: &str, patch: Map<String, Value>, reason: Option<&str>) -> Result<()> {
        if !GENERIC.contains(&table) {
            return Err(format!("'{table}' cannot be edited directly"));
        }
        if let Some(k) = patch.keys().find(|k| ["id", "created_at"].contains(&k.as_str())) {
            return Err(format!("{table}.{k} cannot be changed"));
        }
        if table == "cohort" {
            let frozen: Option<i64> = self.conn.query_row("SELECT frozen_at FROM cohort WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(db)?.flatten();
            if frozen.is_some() {
                return Err("cohort is frozen; create a new cohort to change its definition".into());
            }
        }
        let (tx, actor) = self.tx()?;
        let cols = columns(&tx, table)?;
        let old = rows(&tx, &format!("SELECT * FROM {table} WHERE id = ?1"), &[Sql::Text(id.into())])?.into_iter().next().ok_or_else(|| format!("{table} '{id}' not found"))?;
        let mut sets = vec![];
        let mut vals = vec![];
        let mut diff = Map::new();
        for (k, v) in &patch {
            if !cols.contains(k) {
                return Err(format!("{table} has no field '{k}'"));
            }
            sets.push(format!("{k} = ?{}", vals.len() + 1));
            vals.push(to_sql(k, v)?);
            diff.insert(k.clone(), json!([old.get(k).cloned().unwrap_or(Value::Null), v]));
        }
        if sets.is_empty() {
            return Ok(());
        }
        vals.push(Sql::Text(id.into()));
        tx.execute(&format!("UPDATE {table} SET {} WHERE id = ?{}", sets.join(", "), vals.len()), params_from_iter(&vals)).map_err(|e| friendly(table, e))?;
        audit(&tx, &actor, table, id, "update", &Value::Object(diff), reason)?;
        tx.commit().map_err(db)?;
        Ok(())
    }

    /// Rows of a table, optionally filtered by one column equality.
    pub fn list(&self, table: &str, filter: Option<(&str, &str)>) -> Result<Vec<Map<String, Value>>> {
        if !READABLE.contains(&table) {
            return Err(format!("unknown table '{table}'"));
        }
        match filter {
            None => rows(&self.conn, &format!("SELECT * FROM {table}"), &[]),
            Some((col, v)) => {
                if !columns(&self.conn, table)?.iter().any(|c| c == col) {
                    return Err(format!("{table} has no field '{col}'"));
                }
                rows(&self.conn, &format!("SELECT * FROM {table} WHERE {col} = ?1"), &[Sql::Text(v.into())])
            }
        }
    }

    pub fn get(&self, table: &str, id: &str) -> Result<Map<String, Value>> {
        self.list(table, Some(("id", id)))?.into_iter().next().ok_or_else(|| format!("{table} '{id}' not found"))
    }

    /// Put an animal in a cage, closing its current stay. Returns warnings (capacity, sex mixing).
    pub fn move_animal(&mut self, animal_id: &str, cage_id: &str, reason: Option<&str>) -> Result<Vec<String>> {
        let (tx, actor) = self.tx()?;
        let warnings = Self::move_in(&tx, &actor, animal_id, cage_id, reason)?;
        tx.commit().map_err(db)?;
        Ok(warnings)
    }

    fn move_in(tx: &Transaction, actor: &str, animal_id: &str, cage_id: &str, reason: Option<&str>) -> Result<Vec<String>> {
        let (cap, closed, kind): (i64, Option<String>, String) = tx.query_row("SELECT capacity, closed_on, kind FROM cage WHERE id = ?1", [cage_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|_| "cage not found".to_string())?;
        if closed.is_some() {
            return Err("cage is closed".into());
        }
        let (sex, dead): (String, Option<String>) = tx.query_row("SELECT sex, death_date FROM animal WHERE id = ?1", [animal_id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|_| "animal not found".to_string())?;
        if dead.is_some() {
            return Err("animal is recorded as dead".into());
        }
        let current: Option<(String, String)> = tx.query_row("SELECT id, cage_id FROM cage_assignment WHERE animal_id = ?1 AND end_at IS NULL", [animal_id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(db)?;
        if current.as_ref().is_some_and(|(_, c)| c == cage_id) {
            return Ok(vec!["animal is already in this cage".into()]);
        }
        let now = now_ms();
        if let Some((aid, from)) = &current {
            tx.execute("UPDATE cage_assignment SET end_at = ?1 WHERE id = ?2", params![now, aid]).map_err(db)?;
            audit(tx, actor, "cage_assignment", aid, "update", &json!({ "end_at": [null, now], "to_cage": cage_id, "from_cage": from }), reason)?;
        }
        let mut f = Map::new();
        for (k, v) in [("animal_id", json!(animal_id)), ("cage_id", json!(cage_id)), ("start_at", json!(now)), ("reason", json!(reason)), ("by_user", json!(actor))] {
            f.insert(k.into(), v);
        }
        insert_row(tx, actor, "cage_assignment", f).map_err(|e| e.replace("cage_assignment: ", ""))?;
        let occupants: Vec<String> = {
            let mut st = tx.prepare("SELECT a.sex FROM cage_assignment c JOIN animal a ON a.id = c.animal_id WHERE c.cage_id = ?1 AND c.end_at IS NULL").map_err(db)?;
            st.query_map([cage_id], |r| r.get(0)).map_err(db)?.collect::<std::result::Result<_, _>>().map_err(db)?
        };
        let mut w = vec![];
        if occupants.len() as i64 > cap {
            w.push(format!("cage over capacity: {} animals, capacity {cap}", occupants.len()));
        }
        if kind != "breeding" && sex != "U" && occupants.iter().any(|s| s != &sex && s != "U") {
            w.push("males and females housed together in a non-breeding cage".into());
        }
        Ok(w)
    }

    /// Wean a litter: creates every pup and its cage stay in one transaction.
    pub fn wean(&mut self, litter_id: &str, weaned_on: &str, pups: &[Pup]) -> Result<Vec<String>> {
        if days(weaned_on).is_none() {
            return Err(format!("'{weaned_on}' is not a date (YYYY-MM-DD)"));
        }
        if pups.is_empty() {
            return Err("no pups to wean".into());
        }
        let (tx, actor) = self.tx()?;
        let (born_on, already, pair): (String, Option<String>, String) = tx.query_row("SELECT born_on, weaned_on, breeding_pair_id FROM litter WHERE id = ?1", [litter_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|_| "litter not found".to_string())?;
        if already.is_some() {
            return Err("litter is already weaned".into());
        }
        if weaned_on < born_on.as_str() {
            return Err("weaning date is before birth".into());
        }
        // Pups inherit the sire's line and protocol.
        let (line, protocol): (Option<String>, String) = tx.query_row("SELECT a.line_id, a.protocol_id FROM breeding_pair b JOIN animal a ON a.id = b.male_id WHERE b.id = ?1", [&pair], |r| Ok((r.get(0)?, r.get(1)?))).map_err(db)?;
        Self::check_protocol(&tx, Some(&json!(protocol)), weaned_on)?;
        let mut ids = vec![];
        for p in pups {
            if !["M", "F", "U"].contains(&p.sex.as_str()) {
                return Err(format!("pup {}: sex must be M, F or U", p.label));
            }
            let mut f = Map::new();
            for (k, v) in [("label", json!(p.label)), ("sex", json!(p.sex)), ("line_id", json!(line)), ("dob", json!(born_on)), ("source", json!("bred")), ("litter_id", json!(litter_id)), ("protocol_id", json!(protocol)), ("ear_tag", json!(p.ear_tag)), ("owner", json!(actor))] {
                f.insert(k.into(), v);
            }
            let id = insert_row(&tx, &actor, "animal", f)?;
            Self::move_in(&tx, &actor, &id, &p.cage_id, Some("wean"))?;
            ids.push(id);
        }
        tx.execute("UPDATE litter SET weaned_on = ?1 WHERE id = ?2", params![weaned_on, litter_id]).map_err(db)?;
        audit(&tx, &actor, "litter", litter_id, "update", &json!({ "weaned_on": [null, weaned_on], "pups": ids.len() }), None)?;
        tx.commit().map_err(db)?;
        Ok(ids)
    }

    /// Record a stereotaxic surgery with its frame, injections and implants atomically.
    pub fn record_surgery(&mut self, s: Surgery) -> Result<String> {
        let (tx, actor) = self.tx()?;
        let mut proc = s.procedure;
        proc.entry("kind").or_insert(json!("stereotaxic_surgery"));
        let on = proc.get("performed_on").and_then(Value::as_str).unwrap_or("").to_string();
        Self::check_protocol(&tx, proc.get("protocol_id"), &on)?;
        let pid = insert_row(&tx, &actor, "procedure", proc)?;
        let mut frame = s.frame;
        frame.insert("procedure_id".into(), json!(pid));
        insert_row(&tx, &actor, "surgery_frame", frame)?;
        for mut x in s.injections {
            x.insert("procedure_id".into(), json!(pid));
            insert_row(&tx, &actor, "injection", x)?;
        }
        for mut x in s.implants {
            x.insert("procedure_id".into(), json!(pid));
            insert_row(&tx, &actor, "implant", x)?;
        }
        tx.commit().map_err(db)?;
        Ok(pid)
    }

    /// Exclude an animal or session (optionally only from one cohort). Exclusions are revoked, never deleted.
    pub fn exclude(&mut self, scope: &str, target_id: &str, cohort_id: Option<&str>, reason: &str, detail: Option<&str>) -> Result<String> {
        let mut f = Map::new();
        for (k, v) in [("scope", json!(scope)), ("target_id", json!(target_id)), ("cohort_id", json!(cohort_id)), ("reason", json!(reason)), ("detail", json!(detail)), ("decided_by", json!(self.actor)), ("decided_at", json!(now_ms()))] {
            f.insert(k.into(), v);
        }
        self.insert("exclusion", f)
    }

    /// Add an animal to a cohort with its between-subject group.
    pub fn add_cohort_member(&mut self, cohort_id: &str, animal_id: &str, group: &str, allocation: &str, blind_code: Option<&str>) -> Result<()> {
        self.ensure_unfrozen(cohort_id)?;
        let (tx, actor) = self.tx()?;
        let mut f = Map::new();
        for (k, v) in [("cohort_id", json!(cohort_id)), ("animal_id", json!(animal_id)), ("group_name", json!(group)), ("allocation", json!(allocation)), ("blind_code", json!(blind_code))] {
            f.insert(k.into(), v);
        }
        insert_row(&tx, &actor, "cohort_member", f)?;
        tx.commit().map_err(db)
    }

    /// Include a session in a cohort under a within-subject condition. The session's animal must be a member.
    pub fn add_cohort_session(&mut self, cohort_id: &str, session_id: &str, condition: &str) -> Result<()> {
        self.ensure_unfrozen(cohort_id)?;
        let member: bool = self
            .conn
            .query_row("SELECT count(*) FROM session s JOIN cohort_member m ON m.animal_id = s.animal_id AND m.cohort_id = ?1 WHERE s.id = ?2", [cohort_id, session_id], |r| r.get::<_, i64>(0))
            .map_err(db)?
            > 0;
        if !member {
            return Err("add the session's animal to the cohort first".into());
        }
        let (tx, actor) = self.tx()?;
        let mut f = Map::new();
        for (k, v) in [("cohort_id", json!(cohort_id)), ("session_id", json!(session_id)), ("condition", json!(condition))] {
            f.insert(k.into(), v);
        }
        insert_row(&tx, &actor, "cohort_session", f)?;
        tx.commit().map_err(db)
    }

    fn ensure_unfrozen(&self, cohort_id: &str) -> Result<()> {
        let frozen: Option<Option<i64>> = self.conn.query_row("SELECT frozen_at FROM cohort WHERE id = ?1", [cohort_id], |r| r.get(0)).optional().map_err(db)?;
        match frozen {
            None => Err("cohort not found".into()),
            Some(Some(_)) => Err("cohort is frozen".into()),
            Some(None) => Ok(()),
        }
    }

    /// Member rows (animal, group, condition, session, block) with exclusions applied.
    fn cohort_rows(&self, cohort_id: &str) -> Result<Vec<Map<String, Value>>> {
        rows(
            &self.conn,
            "SELECT a.label AS animal, m.group_name, m.blind_code, cs.condition, s.id AS session_id, s.block_path, s.block_digest, s.signal_store
             FROM cohort_session cs
             JOIN session s ON s.id = cs.session_id
             JOIN cohort_member m ON m.cohort_id = cs.cohort_id AND m.animal_id = s.animal_id
             JOIN animal a ON a.id = s.animal_id
             WHERE cs.cohort_id = ?1
               AND NOT EXISTS (SELECT 1 FROM exclusion e WHERE e.revoked_at IS NULL AND (e.cohort_id IS NULL OR e.cohort_id = ?1)
                               AND ((e.scope = 'animal' AND e.target_id = a.id) OR (e.scope = 'session' AND e.target_id = s.id)))
             ORDER BY a.label, cs.condition, s.block_path",
            &[Sql::Text(cohort_id.into())],
        )
    }

    /// Freeze a cohort: membership becomes immutable and its BLAKE3 hash is recorded.
    pub fn freeze_cohort(&mut self, cohort_id: &str) -> Result<String> {
        self.ensure_unfrozen(cohort_id)?;
        let rows = self.cohort_rows(cohort_id)?;
        if rows.is_empty() {
            return Err("cohort has no included sessions".into());
        }
        let hash = blake3::hash(Value::Array(rows.into_iter().map(Value::Object).collect()).to_string().as_bytes()).to_hex().to_string();
        let (tx, actor) = self.tx()?;
        let now = now_ms();
        tx.execute("UPDATE cohort SET frozen_at = ?1, members_hash = ?2 WHERE id = ?3", params![now, hash, cohort_id]).map_err(db)?;
        audit(&tx, &actor, "cohort", cohort_id, "freeze", &json!({ "members_hash": hash }), None)?;
        tx.commit().map_err(db)?;
        Ok(hash)
    }

    pub fn unblind(&mut self, cohort_id: &str, reason: &str) -> Result<()> {
        if reason.trim().is_empty() {
            return Err("unblinding needs a reason".into());
        }
        let (tx, actor) = self.tx()?;
        let n = tx.execute("UPDATE cohort SET unblinded_at = ?1 WHERE id = ?2 AND unblinded_at IS NULL", params![now_ms(), cohort_id]).map_err(db)?;
        if n == 0 {
            return Err("cohort not found or already unblinded".into());
        }
        audit(&tx, &actor, "cohort", cohort_id, "unblind", &json!({}), Some(reason))?;
        tx.commit().map_err(db)
    }

    /// A cohort manifest (TOML) for the analysis engine. Block paths are made
    /// relative to `manifest_dir` when possible. While a blinded cohort is not
    /// unblinded, groups are written as their blind codes.
    pub fn cohort_manifest(&self, cohort_id: &str, data_root: &Path) -> Result<String> {
        let c = self.get("cohort", cohort_id)?;
        let blinded = c["blinding"] != "none" && c["unblinded_at"].is_null();
        let events: Vec<String> = serde_json::from_str(c["events"].as_str().unwrap_or("[]")).map_err(db)?;
        let mut s = String::new();
        let esc = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
        s.push_str(&format!("schema = 1\nname = \"{}\"\n", esc(c["name"].as_str().unwrap_or(""))));
        if let Some(q) = c["question"].as_str() {
            s.push_str(&format!("description = \"{}\"\n", esc(q)));
        }
        s.push_str(&format!("preset = \"{}\"\n", esc(c["preset"].as_str().unwrap_or("grab-ach3.0-tdt"))));
        s.push_str(&format!("events = [{}]\n", events.iter().map(|e| format!("{{ epoc = \"{}\" }}", esc(e))).collect::<Vec<_>>().join(", ")));
        if let Some(h) = c["members_hash"].as_str() {
            s.push_str(&format!("# frozen cohort {cohort_id}, members_hash {h}\n"));
        }
        for r in self.cohort_rows(cohort_id)? {
            let group = if blinded { r["blind_code"].as_str().ok_or("blinded cohort member has no blind_code")? } else { r["group_name"].as_str().unwrap_or("all") };
            let block = r["block_path"].as_str().unwrap_or("");
            let abs = if Path::new(block).is_absolute() { block.to_string() } else { data_root.join(block).display().to_string() };
            s.push_str(&format!(
                "\n[[session]]\nblock = \"{}\"\nanimal = \"{}\"\ngroup = \"{}\"\ncondition = \"{}\"\n",
                esc(&abs),
                esc(r["animal"].as_str().unwrap_or("")),
                esc(group),
                esc(r["condition"].as_str().unwrap_or("session"))
            ));
        }
        Ok(s)
    }

    /// Everything due, derived from the records (no task table to drift out of sync).
    pub fn tasks(&self, today: &str, wean_age_days: i64) -> Result<Vec<Task>> {
        let t = days(today).ok_or("today must be YYYY-MM-DD")?;
        let mut out = vec![];
        let mut push = |due: i64, kind: &str, title: String, entity: &str, id: &str| {
            out.push(Task { due_on: date(due), kind: kind.into(), title, entity: entity.into(), entity_id: id.into(), overdue: due < t });
        };
        for r in rows(&self.conn, "SELECT l.id, l.born_on, d.label FROM litter l JOIN animal d ON d.id = l.dam_id WHERE l.weaned_on IS NULL", &[])? {
            if let Some(b) = days(r["born_on"].as_str().unwrap_or(""))
                && b + wean_age_days - 3 <= t {
                    push(b + wean_age_days, "wean", format!("Wean litter of {}", r["label"].as_str().unwrap_or("?")), "litter", r["id"].as_str().unwrap_or(""));
                }
        }
        // Post-op welfare checks at 1, 2 and 3 days unless a welfare check is recorded that day.
        // Only surgeries from the past week: a missed check from months ago is not actionable.
        for r in rows(
            &self.conn,
            "SELECT p.id, p.performed_on, a.label, a.id AS animal_id FROM procedure p JOIN animal a ON a.id = p.animal_id
             WHERE p.kind = 'stereotaxic_surgery' AND a.death_date IS NULL",
            &[],
        )? {
            let Some(d0) = days(r["performed_on"].as_str().unwrap_or("")).filter(|d| t - d <= 7) else { continue };
            for k in 1..=3 {
                let due = date(d0 + k);
                let done: i64 = self
                    .conn
                    .query_row("SELECT count(*) FROM procedure WHERE animal_id = ?1 AND kind = 'welfare_check' AND performed_on = ?2", [r["animal_id"].as_str().unwrap_or(""), &due], |x| x.get(0))
                    .map_err(db)?;
                if done == 0 && d0 + k <= t + 1 {
                    push(d0 + k, "post_op", format!("Post-op check day {k}: {}", r["label"].as_str().unwrap_or("?")), "animal", r["animal_id"].as_str().unwrap_or(""));
                }
            }
        }
        for r in rows(&self.conn, "SELECT id, number, expires_on FROM protocol WHERE archived_at IS NULL", &[])? {
            if let Some(e) = days(r["expires_on"].as_str().unwrap_or(""))
                && e - t <= 30 {
                    push(e, "protocol", format!("Protocol {} expires", r["number"].as_str().unwrap_or("?")), "protocol", r["id"].as_str().unwrap_or(""));
                }
        }
        for r in rows(
            &self.conn,
            "SELECT c.id, c.label, c.capacity, count(a.id) AS n FROM cage c JOIN cage_assignment a ON a.cage_id = c.id AND a.end_at IS NULL
             WHERE c.closed_on IS NULL GROUP BY c.id HAVING n > c.capacity",
            &[],
        )? {
            push(t, "capacity", format!("Cage {} over capacity ({}/{})", r["label"].as_str().unwrap_or("?"), r["n"], r["capacity"]), "cage", r["id"].as_str().unwrap_or(""));
        }
        for r in rows(&self.conn, "SELECT a.id, a.label, l.weaned_on FROM animal a JOIN litter l ON l.id = a.litter_id WHERE a.death_date IS NULL AND l.weaned_on IS NOT NULL AND NOT EXISTS (SELECT 1 FROM genotype_call g WHERE g.animal_id = a.id)", &[])? {
            if let Some(w) = days(r["weaned_on"].as_str().unwrap_or("")) {
                push(w + 7, "genotype", format!("Genotype {}", r["label"].as_str().unwrap_or("?")), "animal", r["id"].as_str().unwrap_or(""));
            }
        }
        out.sort_by(|a, b| a.due_on.cmp(&b.due_on).then(a.title.cmp(&b.title)));
        Ok(out)
    }

    /// One animal's life in order: birth, cages, genotypes, procedures, sessions, exclusions, notes, death.
    pub fn timeline(&self, animal_id: &str) -> Result<Vec<TimelineItem>> {
        let a = self.get("animal", animal_id)?;
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let ms = |v: &Value| v.as_i64().map(|m| date(m.div_euclid(86_400_000))).unwrap_or_default();
        let mut out = vec![];
        let mut item = |when: String, kind: &str, title: String, detail: String, entity: &str, id: String| {
            out.push(TimelineItem { when, kind: kind.into(), title, detail, entity: entity.into(), entity_id: id });
        };
        if let Some(d) = a["dob"].as_str() {
            item(d.into(), "birth", "Born".into(), s(&a["source"]), "animal", animal_id.into());
        }
        if let Some(d) = a["arrived_on"].as_str() {
            item(d.into(), "arrival", format!("Arrived from {}", s(&a["vendor"])), String::new(), "animal", animal_id.into());
        }
        let id = Sql::Text(animal_id.into());
        for r in rows(&self.conn, "SELECT ca.id, ca.start_at, ca.end_at, ca.reason, c.label FROM cage_assignment ca JOIN cage c ON c.id = ca.cage_id WHERE ca.animal_id = ?1", std::slice::from_ref(&id))? {
            item(ms(&r["start_at"]), "cage", format!("Into cage {}", s(&r["label"])), s(&r["reason"]), "cage_assignment", s(&r["id"]));
        }
        for r in rows(&self.conn, "SELECT id, allele, zygosity, basis, called_at FROM genotype_call WHERE animal_id = ?1 AND superseded_by IS NULL", std::slice::from_ref(&id))? {
            item(ms(&r["called_at"]), "genotype", format!("{}: {}", s(&r["allele"]), s(&r["zygosity"])), s(&r["basis"]), "genotype_call", s(&r["id"]));
        }
        for r in rows(&self.conn, "SELECT id, kind, performed_on, notes FROM procedure WHERE animal_id = ?1", std::slice::from_ref(&id))? {
            let mut detail = s(&r["notes"]);
            if r["kind"] == "stereotaxic_surgery" {
                let pid = Sql::Text(s(&r["id"]));
                let parts: Vec<String> = rows(&self.conn, "SELECT i.target_region, i.hemisphere, v.construct FROM injection i JOIN virus_lot v ON v.id = i.virus_lot_id WHERE i.procedure_id = ?1", std::slice::from_ref(&pid))?
                    .iter()
                    .map(|x| format!("{} → {} {}", s(&x["construct"]), s(&x["target_region"]), s(&x["hemisphere"])))
                    .chain(rows(&self.conn, "SELECT kind, target_region, hemisphere FROM implant WHERE procedure_id = ?1", &[pid])?.iter().map(|x| format!("{} in {} {}", s(&x["kind"]).replace('_', " "), s(&x["target_region"]), s(&x["hemisphere"]))))
                    .collect();
                detail = parts.join("; ");
            }
            item(s(&r["performed_on"]), "procedure", s(&r["kind"]).replace('_', " "), detail, "procedure", s(&r["id"]));
        }
        for r in rows(&self.conn, "SELECT id, block_name, started_unix, paradigm, condition FROM session WHERE animal_id = ?1", std::slice::from_ref(&id))? {
            let when = r["started_unix"].as_f64().map(|u| date((u / 86_400.0).floor() as i64)).unwrap_or_default();
            item(when, "session", format!("{} · {}", s(&r["paradigm"]), s(&r["block_name"])), s(&r["condition"]), "session", s(&r["id"]));
        }
        for r in rows(&self.conn, "SELECT id, reason, detail, decided_at FROM exclusion WHERE scope = 'animal' AND target_id = ?1 AND revoked_at IS NULL", std::slice::from_ref(&id))? {
            item(ms(&r["decided_at"]), "exclusion", format!("Excluded: {}", s(&r["reason"]).replace('_', " ")), s(&r["detail"]), "exclusion", s(&r["id"]));
        }
        for r in rows(&self.conn, "SELECT id, body_md, created_at FROM note WHERE entity = 'animal' AND entity_id = ?1", &[id])? {
            item(ms(&r["created_at"]), "note", "Note".into(), s(&r["body_md"]), "note", s(&r["id"]));
        }
        if let Some(d) = a["death_date"].as_str() {
            item(d.into(), "death", "Died".into(), s(&a["death_reason"]), "animal", animal_id.into());
        }
        out.sort_by(|x, y| x.when.cmp(&y.when));
        Ok(out)
    }

    /// Recompute the audit hash chain. Returns the number of verified rows.
    pub fn verify_audit(&self) -> Result<usize> {
        let mut prev = "genesis".to_string();
        let all = rows(&self.conn, "SELECT * FROM audit ORDER BY seq", &[])?;
        for r in &all {
            let s = |k: &str| r[k].as_str().unwrap_or("").to_string();
            let h = chain_hash(&prev, r["at"].as_i64().unwrap_or(0), &s("actor"), &s("entity"), &s("entity_id"), &s("op"), &s("diff"), r["reason"].as_str());
            if s("prev_hash") != prev || s("hash") != h {
                return Err(format!("audit chain broken at seq {}", r["seq"]));
            }
            prev = h;
        }
        Ok(all.len())
    }

    /// Current occupants per open cage: `cage_id → [animal_id]`.
    pub fn occupancy(&self) -> Result<BTreeMap<String, Vec<String>>> {
        let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in rows(&self.conn, "SELECT cage_id, animal_id FROM cage_assignment WHERE end_at IS NULL", &[])? {
            m.entry(r["cage_id"].as_str().unwrap_or("").into()).or_default().push(r["animal_id"].as_str().unwrap_or("").into());
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn lab() -> (Lab, String, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("simplefp-lab-{}-{}.sqlite", std::process::id(), new_id()));
        let mut lab = Lab::open(&p, "DA").unwrap();
        let proto = lab.insert("protocol", obj(json!({"number": "2025-001", "title": "ACh and feeding", "approved_from": "2025-01-01", "expires_on": "2099-01-01", "species": "Mus musculus", "approved_count": 200}))).unwrap();
        (lab, proto, p)
    }

    #[test]
    fn dates_roundtrip() {
        for d in ["1970-01-01", "2000-02-29", "2026-10-25", "1999-12-31"] {
            assert_eq!(date(days(d).unwrap()), d);
        }
        assert_eq!(days("2026-10-25").unwrap() - days("2026-10-04").unwrap(), 21);
        assert!(days("2025-02-29").is_none() && days("2025-13-01").is_none() && days("25-1-1").is_none());
    }

    #[test]
    fn one_open_cage_stay_and_moves_keep_history() {
        let (mut lab, proto, _) = lab();
        let c1 = lab.insert("cage", obj(json!({"label": "C-001", "kind": "holding", "protocol_id": proto, "opened_on": "2026-10-01", "capacity": 1}))).unwrap();
        let c2 = lab.insert("cage", obj(json!({"label": "C-002", "kind": "holding", "protocol_id": proto, "opened_on": "2026-10-01"}))).unwrap();
        let a = lab.insert("animal", obj(json!({"label": "A-1", "sex": "M", "source": "vendor", "vendor": "JAX", "arrived_on": "2026-10-01", "protocol_id": proto}))).unwrap();
        let b = lab.insert("animal", obj(json!({"label": "A-2", "sex": "F", "source": "vendor", "vendor": "JAX", "arrived_on": "2026-10-01", "protocol_id": proto}))).unwrap();
        assert!(lab.move_animal(&a, &c1, Some("arrival")).unwrap().is_empty());
        let w = lab.move_animal(&b, &c1, None).unwrap();
        assert!(w.iter().any(|w| w.contains("capacity")) && w.iter().any(|w| w.contains("females")), "{w:?}");
        lab.move_animal(&a, &c2, Some("move")).unwrap();
        let stays = lab.list("cage_assignment", Some(("animal_id", &a))).unwrap();
        assert_eq!(stays.len(), 2);
        assert_eq!(stays.iter().filter(|s| s["end_at"].is_null()).count(), 1);
        // The partial unique index is the backstop if anything bypasses move_animal.
        let raw = lab.conn.execute("INSERT INTO cage_assignment (id, animal_id, cage_id, start_at, by_user) VALUES ('x', ?1, ?2, 0, 'DA')", [&a, &c1]);
        assert!(raw.is_err());
        assert!(lab.insert("cage_assignment", Map::new()).is_err());
    }

    #[test]
    fn weaning_is_atomic_and_inherits() {
        let (mut lab, proto, _) = lab();
        let line = lab.insert("line", obj(json!({"name": "ChAT-Cre", "background": "C57BL/6J"}))).unwrap();
        let bc = lab.insert("cage", obj(json!({"label": "B-01", "kind": "breeding", "protocol_id": proto, "opened_on": "2026-09-01"}))).unwrap();
        let wc = lab.insert("cage", obj(json!({"label": "W-01", "kind": "holding", "protocol_id": proto, "opened_on": "2026-09-01"}))).unwrap();
        let m = lab.insert("animal", obj(json!({"label": "S-1", "sex": "M", "line_id": line, "source": "vendor", "arrived_on": "2026-08-01", "protocol_id": proto}))).unwrap();
        let f = lab.insert("animal", obj(json!({"label": "D-1", "sex": "F", "line_id": line, "source": "vendor", "arrived_on": "2026-08-01", "protocol_id": proto}))).unwrap();
        let pair = lab.insert("breeding_pair", obj(json!({"cage_id": bc, "kind": "pair", "male_id": m, "set_up_on": "2026-09-01"}))).unwrap();
        let litter = lab.insert("litter", obj(json!({"breeding_pair_id": pair, "dam_id": f, "born_on": "2026-10-01", "born_count": 3}))).unwrap();
        assert!(lab.tasks("2026-10-20", 21).unwrap().iter().any(|t| t.kind == "wean"));
        // A duplicate label in the batch must roll back the whole weaning.
        let bad = [Pup { label: "P-1".into(), sex: "M".into(), ear_tag: None, cage_id: wc.clone() }, Pup { label: "P-1".into(), sex: "F".into(), ear_tag: None, cage_id: wc.clone() }];
        assert!(lab.wean(&litter, "2026-10-22", &bad).is_err());
        assert!(lab.list("animal", Some(("litter_id", &litter))).unwrap().is_empty());
        assert!(lab.get("litter", &litter).unwrap()["weaned_on"].is_null());
        let ok = [Pup { label: "P-1".into(), sex: "M".into(), ear_tag: None, cage_id: wc.clone() }, Pup { label: "P-2".into(), sex: "M".into(), ear_tag: None, cage_id: wc.clone() }];
        let ids = lab.wean(&litter, "2026-10-22", &ok).unwrap();
        assert_eq!(ids.len(), 2);
        let pup = lab.get("animal", &ids[0]).unwrap();
        assert_eq!((pup["dob"].as_str(), pup["line_id"].as_str()), (Some("2026-10-01"), Some(line.as_str())));
        assert_eq!(lab.occupancy().unwrap()[&wc].len(), 2);
        assert!(lab.tasks("2026-10-30", 21).unwrap().iter().any(|t| t.kind == "genotype"));
    }

    #[test]
    fn audit_chain_detects_tampering_and_rejects_edits() {
        let (mut lab, proto, path) = lab();
        lab.update("protocol", &proto, obj(json!({"title": "ACh, feeding and social"})), Some("amendment")).unwrap();
        assert_eq!(lab.verify_audit().unwrap(), 2);
        assert!(lab.conn.execute("UPDATE audit SET actor = 'X'", []).is_err());
        assert!(lab.conn.execute("DELETE FROM audit", []).is_err());
        // Tampering below the API (dropping the trigger) is still detected by the chain.
        lab.conn.execute_batch("DROP TRIGGER audit_no_update; UPDATE audit SET diff = '{}' WHERE seq = 1;").unwrap();
        assert!(lab.verify_audit().is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn expired_protocol_blocks_new_work() {
        let (mut lab, _, _) = lab();
        let old = lab.insert("protocol", obj(json!({"number": "2019-9", "title": "old", "approved_from": "2019-01-01", "expires_on": "2020-01-01", "species": "Mus musculus", "approved_count": 1}))).unwrap();
        let e = lab.insert("cage", obj(json!({"label": "Z", "kind": "holding", "protocol_id": old, "opened_on": "2026-10-01"}))).unwrap_err();
        assert!(e.contains("expired"), "{e}");
        assert!(lab.insert("animal", obj(json!({"label": "A", "sex": "Q", "source": "vendor", "arrived_on": "2026-10-01", "protocol_id": old}))).is_err());
        assert!(lab.insert("cage", obj(json!({"label": "Z", "kind": "holding", "protocol_id": old, "opened_on": "not-a-date"}))).unwrap_err().contains("date"));
        assert!(lab.insert("cage", obj(json!({"label": "Z", "nope": 1}))).unwrap_err().contains("no field"));
    }

    #[test]
    fn cohort_freeze_exclusion_and_blinded_manifest() {
        let (mut lab, proto, _) = lab();
        let mut sessions = vec![];
        let mut animals = vec![];
        for (i, g) in ["ChAT", "ChAT", "WT"].iter().enumerate() {
            let a = lab.insert("animal", obj(json!({"label": format!("A-{i}"), "sex": "M", "source": "vendor", "arrived_on": "2026-01-01", "protocol_id": proto}))).unwrap();
            let s = lab.insert("session", obj(json!({"animal_id": a, "block_path": format!("blocks/A-{i}"), "block_digest": "x", "block_name": format!("A-{i}"), "started_unix": 1.75e9, "signal_store": "465A", "paradigm": "refeed"}))).unwrap();
            animals.push((a, *g));
            sessions.push(s);
        }
        let c = lab.insert("cohort", obj(json!({"name": "Refeed", "preset": "grab-ach3.0-tdt", "events": "[\"PC0_\"]", "blinding": "assessment", "created_by": "DA"}))).unwrap();
        for (i, (a, g)) in animals.iter().enumerate() {
            lab.add_cohort_member(&c, a, g, "genotype", Some(&format!("B-{i}"))).unwrap();
            lab.add_cohort_session(&c, &sessions[i], "fasted").unwrap();
        }
        lab.exclude("animal", &animals[1].0, Some(&c), "fiber_misplaced", Some("tip in LS")).unwrap();
        let m = lab.cohort_manifest(&c, Path::new("/data")).unwrap();
        assert!(m.contains("group = \"B-0\"") && !m.contains("ChAT"), "{m}");
        assert!(!m.contains("A-1\""), "excluded animal leaked: {m}");
        assert!(m.contains("block = \"/data/blocks/A-0\""));
        lab.freeze_cohort(&c).unwrap();
        assert!(lab.add_cohort_member(&c, &animals[1].0, "WT", "genotype", None).is_err());
        assert!(lab.update("cohort", &c, obj(json!({"name": "x"})), None).is_err());
        lab.unblind(&c, "analysis plan locked").unwrap();
        assert!(lab.cohort_manifest(&c, Path::new("/data")).unwrap().contains("group = \"ChAT\""));
        assert!(lab.verify_audit().unwrap() > 10);
    }
}
