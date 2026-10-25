-- SimpleFP lab store, schema 1. Design: docs/research/colony-management.md §3.
-- History is never overwritten: location, genotype and exclusion are event or
-- interval rows. Every write goes through the engine, which appends to `audit`
-- in the same transaction. Dates are 'YYYY-MM-DD'; instants are unix ms.

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;

CREATE TABLE user (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  initials TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL CHECK (role IN ('pi', 'manager', 'member', 'viewer')),
  active INTEGER NOT NULL DEFAULT 1
) STRICT;

CREATE TABLE protocol (
  id TEXT PRIMARY KEY,
  number TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  approved_from TEXT NOT NULL,
  expires_on TEXT NOT NULL,
  species TEXT NOT NULL,
  approved_count INTEGER NOT NULL CHECK (approved_count >= 0),
  archived_at INTEGER,
  CHECK (expires_on >= approved_from)
) STRICT;

CREATE TABLE line (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  official_symbol TEXT,
  background TEXT NOT NULL,
  source TEXT,
  rrid TEXT,
  archived_at INTEGER
) STRICT;

CREATE TABLE cage (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('holding', 'breeding', 'experimental', 'quarantine')),
  room TEXT,
  rack TEXT,
  position INTEGER,
  capacity INTEGER NOT NULL DEFAULT 5 CHECK (capacity BETWEEN 1 AND 20),
  protocol_id TEXT NOT NULL REFERENCES protocol (id),
  opened_on TEXT NOT NULL,
  closed_on TEXT,
  card_printed_at INTEGER
) STRICT;

CREATE TABLE breeding_pair (
  id TEXT PRIMARY KEY,
  cage_id TEXT NOT NULL REFERENCES cage (id),
  kind TEXT NOT NULL CHECK (kind IN ('pair', 'trio', 'harem', 'timed')),
  male_id TEXT NOT NULL REFERENCES animal (id),
  set_up_on TEXT NOT NULL,
  ended_on TEXT,
  goal TEXT
) STRICT;

CREATE TABLE breeding_female (
  pair_id TEXT NOT NULL REFERENCES breeding_pair (id),
  female_id TEXT NOT NULL REFERENCES animal (id),
  joined_on TEXT NOT NULL,
  left_on TEXT,
  PRIMARY KEY (pair_id, female_id)
) STRICT;

CREATE TABLE litter (
  id TEXT PRIMARY KEY,
  breeding_pair_id TEXT NOT NULL REFERENCES breeding_pair (id),
  dam_id TEXT NOT NULL REFERENCES animal (id),
  born_on TEXT NOT NULL,
  born_count INTEGER CHECK (born_count >= 0),
  weaned_on TEXT,
  notes TEXT
) STRICT;

CREATE TABLE animal (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL UNIQUE,
  species TEXT NOT NULL DEFAULT 'Mus musculus',
  sex TEXT NOT NULL CHECK (sex IN ('M', 'F', 'U')),
  line_id TEXT REFERENCES line (id),
  dob TEXT,
  dob_estimated INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL CHECK (source IN ('bred', 'vendor')),
  litter_id TEXT REFERENCES litter (id),
  vendor TEXT,
  arrived_on TEXT,
  protocol_id TEXT NOT NULL REFERENCES protocol (id),
  owner TEXT,
  ear_tag TEXT UNIQUE,
  death_date TEXT,
  death_reason TEXT,
  notes TEXT,
  created_at INTEGER NOT NULL,
  CHECK (source <> 'bred' OR litter_id IS NOT NULL),
  CHECK (dob IS NOT NULL OR arrived_on IS NOT NULL)
) STRICT;

CREATE TABLE genotype_call (
  id TEXT PRIMARY KEY,
  animal_id TEXT NOT NULL REFERENCES animal (id),
  allele TEXT NOT NULL,
  zygosity TEXT NOT NULL CHECK (zygosity IN ('hom', 'het', 'hemi', 'wt', 'unknown')),
  basis TEXT NOT NULL CHECK (basis IN ('assay', 'inferred_from_parents', 'vendor_import', 'assumed_line')),
  called_at INTEGER NOT NULL,
  called_by TEXT NOT NULL,
  superseded_by TEXT REFERENCES genotype_call (id)
) STRICT;

-- The only source of an animal's location: one interval per stay.
CREATE TABLE cage_assignment (
  id TEXT PRIMARY KEY,
  animal_id TEXT NOT NULL REFERENCES animal (id),
  cage_id TEXT NOT NULL REFERENCES cage (id),
  start_at INTEGER NOT NULL,
  end_at INTEGER,
  reason TEXT,
  by_user TEXT NOT NULL,
  CHECK (end_at IS NULL OR end_at >= start_at)
) STRICT;
CREATE UNIQUE INDEX one_open_assignment ON cage_assignment (animal_id) WHERE end_at IS NULL;

CREATE TABLE virus_lot (
  id TEXT PRIMARY KEY,
  construct TEXT NOT NULL,
  serotype TEXT NOT NULL,
  titer_vg_per_ml REAL NOT NULL CHECK (titer_vg_per_ml > 0),
  source TEXT NOT NULL,
  lot TEXT,
  received_on TEXT
) STRICT;

CREATE TABLE procedure (
  id TEXT PRIMARY KEY,
  animal_id TEXT NOT NULL REFERENCES animal (id),
  kind TEXT NOT NULL CHECK (kind IN ('stereotaxic_surgery', 'injection_nonsurgical', 'implant_removal', 'perfusion',
    'histology', 'euthanasia', 'weighing', 'welfare_check', 'treatment', 'behavior_training', 'other')),
  performed_on TEXT NOT NULL,
  operators TEXT NOT NULL,
  protocol_id TEXT NOT NULL REFERENCES protocol (id),
  anesthesia TEXT,
  analgesia TEXT,
  weight_pre_g REAL CHECK (weight_pre_g > 0),
  weight_post_g REAL CHECK (weight_post_g > 0),
  outcome TEXT CHECK (outcome IN ('recovered', 'non_recovery', 'died')),
  notes TEXT,
  CHECK (kind <> 'stereotaxic_surgery' OR weight_pre_g IS NOT NULL)
) STRICT;

CREATE TABLE surgery_frame (
  procedure_id TEXT PRIMARY KEY REFERENCES procedure (id),
  reference TEXT NOT NULL CHECK (reference IN ('bregma', 'lambda')),
  skull_flat INTEGER NOT NULL,
  atlas TEXT
) STRICT;

-- Sign convention: AP + anterior, ML + right, DV measured from dv_reference (positive = ventral).
CREATE TABLE injection (
  id TEXT PRIMARY KEY,
  procedure_id TEXT NOT NULL REFERENCES procedure (id),
  virus_lot_id TEXT NOT NULL REFERENCES virus_lot (id),
  target_region TEXT NOT NULL,
  hemisphere TEXT NOT NULL CHECK (hemisphere IN ('L', 'R')),
  ap_mm REAL NOT NULL,
  ml_mm REAL NOT NULL,
  dv_mm REAL NOT NULL,
  dv_reference TEXT NOT NULL CHECK (dv_reference IN ('skull', 'dura', 'brain')),
  volume_nl REAL NOT NULL CHECK (volume_nl > 0),
  rate_nl_per_min REAL,
  angle_deg REAL,
  dilution REAL CHECK (dilution > 0 AND dilution <= 1)
) STRICT;

CREATE TABLE implant (
  id TEXT PRIMARY KEY,
  procedure_id TEXT NOT NULL REFERENCES procedure (id),
  kind TEXT NOT NULL CHECK (kind IN ('optic_fiber', 'cannula', 'headplate', 'electrode')),
  target_region TEXT NOT NULL,
  hemisphere TEXT NOT NULL CHECK (hemisphere IN ('L', 'R', 'midline')),
  ap_mm REAL NOT NULL,
  ml_mm REAL NOT NULL,
  dv_mm REAL NOT NULL,
  dv_reference TEXT NOT NULL CHECK (dv_reference IN ('skull', 'dura', 'brain')),
  angle_deg REAL,
  core_um REAL,
  na REAL,
  fiber_serial TEXT,
  CHECK (kind <> 'optic_fiber' OR (core_um IS NOT NULL AND na IS NOT NULL))
) STRICT;

CREATE TABLE histology (
  id TEXT PRIMARY KEY,
  implant_id TEXT NOT NULL REFERENCES implant (id),
  verified_region TEXT NOT NULL,
  verified_ap_mm REAL,
  verified_ml_mm REAL,
  verified_dv_mm REAL,
  placement_ok TEXT NOT NULL CHECK (placement_ok IN ('yes', 'no', 'borderline')),
  expression_ok TEXT NOT NULL CHECK (expression_ok IN ('yes', 'no', 'partial')),
  verified_blind INTEGER NOT NULL,
  verifier TEXT NOT NULL,
  verified_at INTEGER NOT NULL
) STRICT;

CREATE TABLE session (
  id TEXT PRIMARY KEY,
  animal_id TEXT NOT NULL REFERENCES animal (id),
  block_path TEXT NOT NULL,
  block_digest TEXT NOT NULL,
  block_name TEXT NOT NULL,
  started_unix REAL NOT NULL,
  signal_store TEXT NOT NULL,
  control_store TEXT,
  implant_id TEXT REFERENCES implant (id),
  paradigm TEXT NOT NULL,
  condition TEXT,
  rig TEXT,
  experimenters TEXT,
  qc_status TEXT NOT NULL DEFAULT 'unreviewed' CHECK (qc_status IN ('unreviewed', 'ok', 'flagged')),
  UNIQUE (block_path, signal_store)
) STRICT;

CREATE TABLE video (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES session (id),
  path TEXT NOT NULL,
  digest TEXT NOT NULL,
  sync TEXT NOT NULL CHECK (sync IN ('tdt_cam_epoc', 'synapse_timestamps', 'manual_offset')),
  sync_store TEXT,
  offset_s REAL,
  UNIQUE (session_id, path)
) STRICT;

CREATE TABLE cohort (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  question TEXT,
  preset TEXT NOT NULL,
  events TEXT NOT NULL,             -- JSON array of TDT epoc store names
  blinding TEXT NOT NULL DEFAULT 'none' CHECK (blinding IN ('none', 'allocation', 'conduct', 'assessment')),
  unblinded_at INTEGER,
  frozen_at INTEGER,
  members_hash TEXT,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE cohort_member (
  cohort_id TEXT NOT NULL REFERENCES cohort (id),
  animal_id TEXT NOT NULL REFERENCES animal (id),
  group_name TEXT NOT NULL,
  allocation TEXT NOT NULL CHECK (allocation IN ('genotype', 'randomized', 'alternating', 'investigator')),
  blind_code TEXT,
  PRIMARY KEY (cohort_id, animal_id)
) STRICT;

CREATE TABLE cohort_session (
  cohort_id TEXT NOT NULL REFERENCES cohort (id),
  session_id TEXT NOT NULL REFERENCES session (id),
  condition TEXT NOT NULL,
  PRIMARY KEY (cohort_id, session_id)
) STRICT;

CREATE TABLE exclusion (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL CHECK (scope IN ('animal', 'session')),
  target_id TEXT NOT NULL,
  cohort_id TEXT REFERENCES cohort (id),
  reason TEXT NOT NULL CHECK (reason IN ('fiber_misplaced', 'no_expression', 'signal_qc', 'health', 'death',
    'procedure_failure', 'behavior_criterion', 'other')),
  detail TEXT,
  decided_by TEXT NOT NULL,
  decided_at INTEGER NOT NULL,
  revoked_at INTEGER
) STRICT;

CREATE TABLE note (
  id TEXT PRIMARY KEY,
  author TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  body_md TEXT NOT NULL,
  entity TEXT,
  entity_id TEXT
) STRICT;

-- Append-only, hash-chained. hash = BLAKE3(prev_hash | at | actor | entity | entity_id | op | diff | reason).
CREATE TABLE audit (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  at INTEGER NOT NULL,
  actor TEXT NOT NULL,
  entity TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  op TEXT NOT NULL CHECK (op IN ('create', 'update', 'archive', 'restore', 'unblind', 'override', 'freeze')),
  diff TEXT NOT NULL,
  reason TEXT,
  prev_hash TEXT NOT NULL,
  hash TEXT NOT NULL
) STRICT;
CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;
CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;
