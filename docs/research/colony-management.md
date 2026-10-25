# Colony management: benchmark and SimpleFP design

Research date: 2026-10-25. Scope: academic mouse colony, cage and experiment-subject software, and what a
photometry/behavior lab needs to link animals to TDT sessions (roadmap v0.2 cohorts, v0.5 lab management).

**How to read the evidence.** Most feature claims come from vendor pages or institutional guides. Independent user
reviews are scarce: Capterra/G2 have no substantive reviews for SoftMouse, PyRAT, Mosaic or mLIMS, Reddit could not be
fetched, and ResearchGate returned 403. Pain points therefore rest mainly on peer-reviewed tool papers (JCMS,
RodentSQL, TopoDB, MyVivarium), whose authors had motives of their own, plus competitor marketing (marked
**[vendor]**). Marks: **[unverified]** means one weak source or my background knowledge; **[bg]** means background
knowledge, not re-checked this session.

---

## 1. Feature matrix

Legend: **Y** documented · **P** partial or paid tier only · **?** not found in the sources (unknown, not proof of
absence) · **N** documented as absent.

| Product (vendor) | Breeding pair/trio | Litters, weaning | Genotyping | Cage cards / barcode / QR | Census | Protocol / IACUC | Procedures / surgery | Health / severity | Roles | Audit trail | Import / export | Mobile | Pricing | Platform |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **SoftMouse.NET** (Locus) | Y, plug and breeder-replacement alerts | Y, wean email alerts | Y, gene/allele dropdowns | Y, cards and stickers; barcode not listed | P (usage stats) | Y (module) | ? (Study Manager module) | ? | Y | P (paid tier per TopoDB 2020) | Y (import tool + export) | Web on tablets; no native app or scanning per a competitor **[vendor]** | Free single user; Premium about $350–1,050/yr academic (1–15 users) **[vendor]** | Cloud web |
| **Mosaic Vivarium** (Virtual Chemistry) | Y, pedigree | Y; weans checked against protocol limits | Y | Y, barcode cards; RFID (2012) | Y, drives per diem | Y, enforced limits (RISe integration at UBC) | P (service requests) | ? | Y (institution SSO) | ? | ? | ? | Enterprise quote | Web, facility-run |
| **JCMS** (JAX) | Y, timed matings | Y | Y | Y, barcodes, 20+ templates | ? | Y (experiment mgmt) | P (data descriptors) | ? | 3 levels | N (planned) | Y, transactional bulk load with rollback | Via middleware only | Free, **now unsupported** | Access/MySQL, Windows |
| **Climb** (RockStep → Instem, Jul 2025) | Y, pedigree, breeding performance | Y | Y | Y | Y | Y | Y, study design, task scheduling | Y, country-agnostic severity (Oct 2025) | Y | Y (strong, per MGH report) | Y (CSV, `;` delimiter) | ? | Enterprise quote | Azure SaaS |
| **PyRAT** (Scionics) | Y, generations, pedigree graphs | Y [unverified detail] | Y, well plates, tube stickers, Transnetyx/CSV import | Y, cards and labels, graphical rack | Y, occupancy | Y, EU 2010/63 authorizations | Y (experiments add-on) | Y, severity for GM lines | Y, plus training gating | Y | Y, API | ? | Quote | Web, hosted or on-prem, 11 languages |
| **mLIMS** (BioInfoRx) | Y, pair or multi-animal | Y, litter survival reports | Y, unlimited genotypes | Y, user-designed cards; barcode census add-on | P (add-on) | Y | Y (experiments) | ? | Y; researchers read-only in facility edition | P (paid tier) | ? | Mobile web | From $360/yr single user to $1,100/yr for 11–15 users (academic) | Cloud web |
| **Topaz Elements** (TOPAZ) | ? | ? | ? | Y (RFID census with Tecniplast, 2009) | Y | Y, protocol review | Y, veterinary management | Y | Y | ? | ? | ? | Enterprise | Cloud or on-prem |
| **tick@lab** (a-tune) | Y, breeding lines | Y | Y; Transnetyx AMI connector | Y, configurable cards | Y, room occupancy | Y, IACUC, Home Office, TSchG | Y, study planning | Y, 3R blackboard | Y | Y [unverified] | Y, API, web services | ? | Enterprise | Web |
| **Labguru** (BioData) Rodents | P (strains, specimens) | ? | ? | P (cages, bulk import) | P (count per cage) | ? | P (treatments: past, current, future) | ? | Y | Y (ELN) | Y (cage import) | ? | Quote; about $180/user/yr listed | Cloud ELN/LIMS |
| **eLabFTW** (Deltablot) | N (generic items) | N | N | P (QR/barcode on items) | N | N | P (experiments as ELN entries) | N | Y, teams and permissions | Y [bg: revisions, RFC 3161 timestamps] | Y | Mobile web | Free AGPL self-host; paid hosting [bg] | PHP web |
| **LabCollector** (AgileBio) | Y, crossings, lineage | Y (birth dates) | ? | Y, barcoded cages | P (occupied/free cages) | ? | P (experiment dates) | P (health comments) | Y | ? | ? | ? | Per-module licence [bg] | Web, on-prem or cloud |
| **MausDB** (Helmholtz) | Y | Y | Y | Y, barcode | ? | ? | Y, phenotyping workflows | ? | N (no admin UI per MyVivarium) | N (TopoDB table) | Y | N | Free GPL; install files reportedly unavailable | Perl web, 2008 |
| **AniBio** (NorayBio) | ? | ? | Y | Y, card-driven elimination | Y | ? | ? | ? | Y | ? | ? | ? | Basic to Premium editions | Web; Tecniplast DVC integration |
| **Transnetyx Colony** (+AMI) | Y; AMI breeder alerts, pedigree draw | Y | Y, order and assign results in-app | Y, iPad barcode scan | ? | ? | ? | ? | Y | ? | ? | **Native iOS, offline** | Free with Transnetyx genotyping (e.g. Cedars-Sinai) | Cloud + iOS |
| **Labii** Mouse Colony (Jan 2025) | Y, multi-breeder mating | Y, auto generation, batch-create mice from litter | ? | P (storage system) | P (cage occupancy) | ? | ? | ? | Y | Y [bg] | ? | ? | Quote | Cloud |
| **Moustra** (2025) **[vendor]** | Y | Y, weaning reminders | Y | Y, barcode scanning | ? | ? | ? | ? | Y | ? | AI-assisted Excel import | Native iOS/Android | Free during beta | Cloud |
| **Colony.care** **[vendor]** | Y, breeding calendar | Y | ? | Y, QR cards printed from browser | Y, per diem | Y | ? | Y | Y | Y, indefinite retention | **CSV import with dry-run and rollback**, CSV/JSON | PWA with scanning | Free up to 100 animals | Cloud |
| **MyVivarium** (U. Dayton, 2025) | Y | Y | P (gel images as files) | Y, QR | N | Y (protocol links) | N | N | 2 tiers | **N (no record versioning, per the authors)** | CSV export | QR on phone | Free; about $6/month server | PHP/MySQL + Raspberry Pi sensors |
| **TopoDB** (UCSF, 2020) | Y | Y | Y | ? | ? | ? | P (experimental data) | ? | Y | Y | Y | ? | Free OSS | Rails/MySQL |
| **RodentSQL** (2018) | Y, pedigree tree | Y | Y | Y, barcode | ? | P (protocols tab) | N | N | AD groups | ? | ? | N | Free GPL | Java desktop |
| **Alyx** (IBL/cortex-lab) | Y, father + mother1 + mother2 (trio) | Y | Y, Allele/Zygosity/GenotypeTest | N (cage is a free-text field) | N | P (protocol_number 1–4) | **Y, Surgery, Session, ProcedureType** | Y, weighing, water restriction, actual severity, cull | Y | Y [bg: django-reversion] | Y, REST API + ONE | N | Free OSS | Django/Postgres |
| **BrainSTEM** (NYU/Copenhagen) | ? | ? | P (genetic line) | ? | N | ? | **Y, virus injections, optic fibers, probes, windows** | ? | Y, private, selective or open sharing | ? | Y, REST, MATLAB/Python | ? | Free (NIH-funded) [unverified] | Web |
| Spreadsheets / paper | Manual | Manual | Manual | Paper | Manual | Manual | Manual | Manual | N | N | Y | Sheets app | Free | Any |

Other entrants seen: **CageSync** (QR/NFC, offline, syncs to Google Sheets, no reviews), **SwiftCENSUS** (iOS
camera-based rack census), **Mouse Breeding Tracker** (iOS dam ledger), **Cayuse Vivarium Operations**,
**AnimalTrax** (Stanford), **ConductScience cage card generator** (free QR cards, no state).

**Not found: iRATco.** I found no product by that name, and AniBio is unrelated (NorayBio, Spain). The name may
be misspelled or refer to an in-house system. Ask whoever suggested it.

### Positioning, in one line each
- **Facility systems** (Mosaic, Topaz, tick@lab, PyRAT, Climb, AniBio) are organized around census, billing,
  protocols and authorizations. Researchers are secondary users, sometimes read-only.
- **Lab colony tools** (SoftMouse, mLIMS, Transnetyx Colony, Labii, Moustra, Colony.care) are organized around
  breeding, genotypes and cages. None models a stereotaxic surgery or links to recorded data.
- **Neuroscience metadata systems** (Alyx, BrainSTEM, AIND schema, NWB ndx-ophys-devices, DataJoint element-animal)
  model surgeries and sessions well, but their colony UX is weak or missing. This is the gap SimpleFP can fill.

---

## 2. Pain points, with evidence

| # | Pain point | Evidence |
|---|---|---|
| P1 | **Paper cards and spreadsheets fail silently.** Rows lose links to parents and genotypes, concurrent edits overwrite each other, and there is no attributable history. Cards get wet, autoclaved or lost. | MGH technical report (vendor-adjacent): https://medinformatics.mgh.harvard.edu/resources/vivarium-colony-management.html · RodentSQL (spreadsheets handle breeding schemes and multiple users poorly): https://pmc.ncbi.nlm.nih.gov/articles/PMC6153456/ · JCMS (paper "does not scale well and is error prone"): https://pmc.ncbi.nlm.nih.gov/articles/PMC2844967 |
| P2 | **Commercial tools are expensive and cumbersome**, with steep learning curves. | TopoDB: https://pmc.ncbi.nlm.nih.gov/articles/PMC7673335 · RodentSQL (same) · MyVivarium (rates mLIMS, SoftMouse and PyRAT as high or very high cost): https://pmc.ncbi.nlm.nih.gov/articles/PMC12621272/ |
| P3 | **Basics sit behind paywalls.** Audit logs, file upload and experiment data storage are paid in SoftMouse and mLIMS. Pricing is per user. | TopoDB Table 2 (2020) · mLIMS pricing: https://bioinforx.com/web/product_mlims_biotech.php · SoftMouse per-user tiers **[vendor]**: https://moustra.com/blog/softmouse-alternative |
| P4 | **Data entry happens late because entry at the cage is hard.** Fast mobile updates were "a major gap". | MyVivarium (2025) · Moustra **[vendor]** · Rutgers SOP: tablets with QR scanning at the rack for births and overcrowding: https://research.rutgers.edu/system/files/2025-11/husbandry-sop-3-21-qr-scanning-births-overcrowded-etc.pdf |
| P5 | **Tools get abandoned or are hard to install.** JCMS is unsupported, Sanger MCMS is archived, MausDB installation files are reportedly unavailable, and one ResearchGate user could not run a tool that needs SQL on a locked-down PC. Vendors consolidate (Instem bought RockStep in July 2025). | https://www.jax.org/research-and-faculty/tools/jax-colony-management-system · MyVivarium Table 1 · https://www.instem.com/instem-announces-acquisition-of-rockstep-solutions/ · ResearchGate thread (403; quoted via search snippet [unverified]) |
| P6 | **The facility's system and the lab's system are separate.** The facility owns census and billing, researchers keep duplicate records, and facility editions make researchers read-only. | mLIMS facility edition: https://bioinforx.com/web/product_mlims_facility.php · UBC Mosaic is the "official record": https://animalcare.ubc.ca/conducting-your-research/managing-your-study-animals-mosaic |
| P7 | **Colony records do not reach experiments or data.** Surgery and session metadata live in notebooks. Integration is the most-requested gap. | MGH report (colony → experiment → publication pipeline, ARRIVE generated from records) · MyVivarium (genotype data "kept separately in lab notebooks") · BrainSTEM exists to solve this: https://world-wide.org/fens-26/brainstem-collaborative-electronic-96039b13 |
| P8 | **Over- and under-breeding.** Surplus animals cost per diem. Missing genotypes delay experiments. | MGH report: 20–40% surplus in a 500-cage colony (**uncited claim**) · Transnetyx AMI and Moustra market cage consolidation for this reason |
| P9 | **Protocol drift.** Breeding continues after IACUC lapse, or numbers exceed approved counts. | MGH report; Mosaic enforces protocol limits on weans and orders (UBC) |
| P10 | **Rigid schemas.** Specialized genotype and phenotype fields cannot be added, and naming is locked to one vendor's nomenclature (JAX). | 2026 roundup cited via search snippet [unverified]; MGH report on JAX Colony |
| P11 | **History is not versioned.** Edits overwrite earlier values. | MyVivarium limitations section; spreadsheets (P1) |
| P12 | **Exclusions and blinding are left to memory.** Fiber misplacement and no expression are standard post-hoc exclusions, but no colony tool records them. ARRIVE 2.0 items 3–5 require a priori criteria, n excluded with reasons, randomization and blinding per stage. AALAS journals adopted ARRIVE 2.0 in January 2025. | https://www.journalmetrics.org/blog/arrive-2-0-animal-research-reporting-guide · inclusion rule example ("included only if both viral expression and cannula placement..."): https://assets-eu.researchsquare.com/files/rs-5724831/v1/3c1bff4417328436909f0571.pdf · 2D histology distorts fiber tips (Kahan 2021, LiGS): https://cris.iucc.ac.il/en/publications/light-guided-sectioning-for-precise-in-situ-localization-and-tiss/ |

### Most-loved or most-copied features
1. A **free tier** for one lab or one user (SoftMouse; Colony.care up to 100 animals; MyVivarium and TopoDB entirely free).
2. **Due-date alerts** for wean, plug check, breeder replacement and genotyping (SoftMouse, Moustra).
3. **A QR or barcode card that opens the record** (MyVivarium, Colony.care, Transnetyx app, Rutgers SOP).
4. **Genotyping results imported** from Transnetyx or CSV and assigned in bulk (PyRAT, Transnetyx Colony).
5. **One action turns a litter into animals** (Labii batch-create) and computes the generation automatically.
6. **Pedigree view** (nearly all).
7. **Import that previews and can be rolled back** (Colony.care dry-run; JCMS transactional rollback).
8. **Offline mobile use** (Transnetyx iOS, CageSync).
9. **Protocol limits enforced at the moment of action** (Mosaic + RISe).
10. **Severity scoring per animal over its lifetime** (Climb 2025, PyRAT, Alyx `actual_severity`).

---

## 3. Recommended data model for SimpleFP

**Design rules**
- **IDs:** a UUIDv7 primary key for every row, plus an immutable human label (`ACh-0142`, `C-031`). The label is
  printed on the card. The QR encodes only `simplefp://a/<uuid>` (no personal or protocol data).
- **History is never overwritten.** Location, genotype calls, group assignment and exclusion are interval or
  event rows. "Current state" is a query.
- **Store:** one SQLite file per lab (WAL, foreign keys ON, `STRICT` tables), owned by `crates/engine`.
  `session.toml` beside each block (roadmap v0.2) stays the portable record. The database indexes and
  cross-checks it, so a block copied to another machine still carries its animal, condition and channel map.
- **Audit:** `AuditEvent` is append-only and written in the same transaction as the change it records (§3.12).
- **Units are fixed** and appear in field names: `_mm`, `_nl`, `_um`, `_g`, `vg_per_ml`. Coordinate references are
  explicit.
- Below, **R** = required, **o** = optional, **D** = derived (computed, not stored).

### 3.1 User
`id` R · `name` R · `initials` R (unique, used on cards) · `email` o · `role` R ∈ {pi, manager, member, viewer} ·
`active` R · `orcid` o. MVP: local single-machine users with no passwords (OS account = actor); real auth comes
with sync. Viewers cannot write. Only managers can delete, and a delete is a soft delete (`archived_at`).

### 3.2 Protocol (IACUC / licence)
`id` R · `number` R · `title` R · `pi_user_id` R · `approved_from` R · `expires_on` R · `species` R ·
`approved_count` R (per protocol; per line later) · `pain_category` o · `allowed_procedures` o (list).
D: `used_count`, `days_to_expiry`. Hard rule: if `expires_on` < today, the app blocks new animals, weans and
procedures under that protocol. Overriding the block requires a typed reason, which goes into the audit log.

### 3.3 Strain / Line / Allele / Genotype
- **Line**: `id` R · `name` R (unique; lab nickname) · `official_symbol` o (MGI nomenclature) · `background_strain`
  R (e.g. C57BL/6J) · `source` o (JAX stock no., Addgene, MMRRC) · `rrid` o · `alleles` (M2M) · `active` R.
- **Allele**: `id` R · `symbol` R (e.g. `Chat-IRES-Cre`) · `type` o ∈ {knock-in, transgene, KO, reporter, other}.
- **GenotypeCall** (one row per animal × allele, versioned): `animal_id` R · `allele_id` R · `zygosity` R ∈ {hom,
  het, hemi, wt, unknown} · `basis` R ∈ {assay, inferred_from_parents, vendor_import, assumed_line} ·
  `assay_id` o · `called_at` R · `called_by` R · `superseded_by` o. Expected genotype is D (from the parents).
  A mismatch between expected and called is flagged.
- **GenotypeAssay**: `id` R · `sample_type` R (tail, ear) · `sampled_on` R · `vendor` o (Transnetyx) · `order_id` o
  · `raw_result` o · `attachment` o (gel image).

### 3.4 Animal
| Field | Req | Notes |
|---|---|---|
| `id`, `label` | R | label immutable, lab-unique |
| `species` | R | default `Mus musculus` |
| `sex` | R | M / F / U (U allowed until weaning) |
| `line_id` | R | or "wild type" line |
| `dob` | R* | *or `dob_estimated` = true with `arrived_on` (vendor animals) |
| `source` | R | bred (`litter_id`) or vendor (`vendor`, `vendor_id`, `arrived_on`) |
| `litter_id` | o | required when source = bred; parents come from the litter |
| `protocol_id` | R | the one under which it is used now; transfers are events |
| `owner_user_id` | R | responsible person |
| `ear_tag` / `tattoo` / `rfid` | o | at least one physical ID recommended; unique when present |
| `status` | D | from events: alive · in_experiment · dead · transferred |
| `death_date`, `death_reason`, `euthanasia_method` | o | set together via a single Euthanasia procedure |
| `severity_actual` | D | max over procedures and welfare checks |
| `tags` | o | free tags (`#pilot`) |

### 3.5 Cage and CageAssignment
- **Cage**: `id`, `label` R · `type` R ∈ {holding, breeding, experimental, quarantine} · `room` o · `rack` o ·
  `position` o (rack/row/column for census order) · `capacity` R (default 5) · `protocol_id` R · `opened_on` R ·
  `closed_on` o · `card_printed_at` o. D: `occupants`, `over_capacity`.
- **CageAssignment** (interval; the only source of location):
  `animal_id` R · `cage_id` R · `from` R (timestamp) · `to` o (null = current) · `reason` o ∈ {wean, move, mating,
  surgery recovery, single house, consolidation} · `by_user` R.
  Constraints: at most one open interval per animal (partial unique index `WHERE to IS NULL`). Moving an animal
  closes the old interval and opens a new one in one transaction.

### 3.6 BreedingPair (mating)
`id` R · `cage_id` R · `type` R ∈ {pair, trio, harem, timed} · `male_id` R · `female_ids` R (1..n, via a join table
with per-female `joined`/`left`) · `set_up_on` R · `ended_on` o · `goal` o (target genotype or line) · `line_id` D.
Events: `plug_seen_on` (timed). D: `litters_count`, `pups_weaned_per_female_month`, `days_since_last_litter`
(drives "replace breeder" alerts), `breeder_age_days`.

### 3.7 Litter
`id` R · `breeding_pair_id` R · `dam_id` R (which female, for trios) · `born_on` R · `born_count` o ·
`alive_count` o · `wean_due` D (= born_on + lab setting, default 21 days. Facilities differ, so this is a setting,
not a hard-coded value) · `weaned_on` o · `weaned_m` / `weaned_f` o · `culled_count` o + reason · `notes` o.
Weaning creates the Animal rows and their CageAssignments in one transaction.

### 3.8 Procedure (surgery and everything done to an animal)
**Procedure** (header): `id` R · `animal_id` R · `type` R ∈ {stereotaxic_surgery, injection_nonsurgical,
implant_removal, perfusion, histology, euthanasia, weighing, welfare_check, treatment, behavior_training, other} ·
`started_at` R · `ended_at` o · `operator_ids` R · `protocol_id` R · `anesthesia` o (agent, dose, route) ·
`analgesia` o (list with times; drives post-op tasks) · `weight_pre_g` o (**R for surgery**) · `weight_post_g` o ·
`outcome` o ∈ {recovered, non_recovery, died} · `severity` o · `notes` o · `template_id` o (copied from).

**SurgeryFrame** (once per surgery): `reference` R ∈ {bregma, lambda} · `skull_flat` R (bool;
bregma–lambda DV difference `bl_dv_diff_mm` o) · `atlas` o (Paxinos & Franklin 2019, Allen CCFv3) ·
`head_angle_deg` o.

**Injection** (0..n per surgery):
| Field | Req | Notes |
|---|---|---|
| `virus_id` → **VirusLot** | R | lot: `construct` R (e.g. `AAV9-hSyn-ACh4.3`), `serotype` R, `titer_vg_per_ml` R, `source` R (Addgene #, WZ Biosciences, core), `lot` o, `received_on` o, `aliquot_location` o, `dilution` o |
| `target_region` | R | controlled list (atlas acronyms) |
| `hemisphere` | R | L / R / bilateral (bilateral = two rows) |
| `ap_mm`, `ml_mm`, `dv_mm` | R | sign convention fixed: AP + anterior, ML + right, DV from `dv_reference` |
| `dv_reference` | R | skull / dura / brain surface |
| `angle_deg`, `angle_plane` | o | for angled approaches |
| `volume_nl` | R | |
| `rate_nl_per_min` | o | |
| `wait_min_after` | o | needle dwell |
| `effective_titer` | D | titer × dilution |

**Implant** (0..n per surgery; fiber, cannula, headplate):
`kind` R ∈ {optic_fiber, cannula, headplate, electrode} · `target_region` R · `hemisphere` R · `ap/ml/dv_mm`
R · `dv_reference` R · `angle_deg` o · for fibers: `core_um` R, `na` R, `length_mm` o, `ferrule` o (1.25/2.5 mm,
ceramic/metal), `manufacturer` o, `part_lot` o, `fiber_id` o (physical serial, also on the TDT channel map) ·
`cement` o · `screws` o. Fiber DV is often set relative to the injection, so the UI offers "relative to
injection #1" and stores the absolute value.

**HistologyVerification** (Procedure type histology, links to its implant and injection):
`perfused_on` R · `sectioning` o (thickness µm, plane) · `stains` o · `images` o (file refs + hash) · per implant:
`verified_ap/ml/dv_mm` o, `verified_region` R, `placement_ok` R ∈ {yes, no, borderline} · per injection:
`expression_ok` R ∈ {yes, no, partial}, `expression_region` o · `verifier_user_id` R · `verified_blind` R (bool) ·
`verified_at` R. This feeds Exclusion (§3.11) and the cohort fiber-tip atlas plot.

### 3.9 Session (link to TDT block + video)
| Field | Req | Notes |
|---|---|---|
| `id` | R | |
| `animal_id` | R | one animal per session row. A multi-animal rig creates N rows that share a block. |
| `block_path` | R | stored relative to a lab "data root" so it survives volume renames |
| `block_digest` | R | BLAKE3 of TSQ (engine already computes digests for `run.json`) |
| `tdt_block_name`, `started_at` | R | read from the block |
| `channel_map` | R | e.g. `465A/405A → implant <uuid>` (L-NAc). This makes multi-site blocks unambiguous. |
| `paradigm` | R | controlled list per lab (fasted refeed, FR1, open field) |
| `condition_id` | o | within-subject condition (drug/vehicle, day 1/day 5) |
| `rig` | o | |
| `experimenter_ids` | R | |
| `video_files` | o | list of path + digest + `sync` ∈ {tdt_cam_epoc, synapse_timestamps, manual_offset_s} |
| `events_files` | o | manual scoring CSVs (`event,time_s`) |
| `days_post_surgery` | D | from the last surgery with an implant on the channel's fiber |
| `weight_g`, `food_restricted` | o | |
| `qc_status` | R | unreviewed / ok / flagged (from `run.json` QC + human review) |
| `analysis_runs` | D | `run.json` paths + preset hash |

### 3.10 Cohort and Group/Condition
- **Cohort**: `id` R · `name` R · `question` o (one sentence, pre-registered aim) · `created_by` R · `selection` R
  (saved filter, e.g. `line=ChAT-Cre AND has_implant(NAc) AND session.paradigm=refeed`) · `frozen_at` o ·
  `members_hash` D. **Freezing** stores the member list with BLAKE3 digests of member IDs and sessions. Analysis
  runs reference the frozen cohort, and their `run.json` records `cohort_id` + `members_hash`.
- **GroupAssignment** (between-subject): `cohort_id` R · `animal_id` R · `group` R (e.g. `KO`, `WT`, `DREADD`) ·
  `allocation` R ∈ {genotype, randomized, alternating, investigator} · `random_seed` o (when randomized) ·
  `blind_code` o (e.g. `B-07`) · `assigned_at` R.
- **Condition** (within-subject): `id` R · `cohort_id` R · `name` R · `order_scheme` o (counterbalanced Latin square).
- **Blinding**: per cohort `blinding` R ∈ {none, allocation, conduct, assessment} (ARRIVE item 5 stages). While
  blinded, the UI shows `blind_code` instead of group anywhere data is displayed. An `Unblind` event (who, when)
  is audited, and Exclusions record whether they were made before or after it.

### 3.11 Exclusion
`id` R · `scope` R ∈ {animal, session, channel, trial_range} · `target_id` R · `cohort_id` o (exclusions can be
cohort-specific) · `reason` R ∈ {fiber_misplaced, no_expression, signal_qc, health, death, procedure_failure,
behavior_criterion, other} · `rule_ref` o (points to an a-priori rule text on the cohort) · `decided_by` R ·
`decided_at` R · `before_unblinding` D · `evidence` o (histology id, run.json QC warning). Excluded items stay in
the data and are filtered out. The methods paragraph reports n excluded per group with reasons (ARRIVE item 3).

### 3.12 Note/Journal and AuditEvent
- **Note**: `id` R · `author_id` R · `created_at` R · `body_md` R · `links` D (parsed from `@ACh-0142`, `#C-031`,
  `!session`) · `attachments` o (file + hash) · `pinned` o. Edits create revisions and the original is kept.
  Notes appear on every linked entity's timeline.
- **AuditEvent** (append-only): `seq` R (monotonic) · `at` R · `actor_id` R · `entity` R · `entity_id` R · `op` R
  ∈ {create, update, archive, restore, unblind, override} · `diff` R (JSON of changed fields: old → new) ·
  `reason` o (R for override) · `prev_hash`, `hash` R (BLAKE3 chain, so tampering shows). Triggers or a
  single engine write path guarantee no write bypasses it. Never editable, never deleted.

### 3.13 Relations (summary)
```
Line 1─* Animal *─1 Litter *─1 BreedingPair *─1 Cage
Animal 1─* CageAssignment *─1 Cage            Animal 1─* GenotypeCall *─1 Allele
Animal 1─* Procedure 1─* {Injection→VirusLot, Implant, HistologyVerification}
Animal 1─* Session (channel_map → Implant)     Cohort *─* Animal (GroupAssignment), Cohort 1─* Condition
Exclusion → {Animal|Session|channel}           Note *─* any entity     every write → AuditEvent
Protocol 1─* {Animal, Cage, Procedure}
```
Tasks (wean due, plug check, post-op checks at 24/48/72 h, analgesia doses, genotype pending, breeder age,
protocol expiry, fiber patency) are **D**: queries over the tables above, so there is no task table to keep in
sync. Completing a task writes the real event (a welfare check, a weaning).

---

## 4. UX principles and flows

### Principles
1. **Record at the moment of action, or the data is fiction** (P4). Every common action must take under 10 s
   and work from a scan or the keyboard.
2. **One timeline per animal** is the core view. Birth, cages, genotype, surgery, sessions, histology, notes and
   death all appear in order, and the photometry traces sit inline.
3. **Never ask for what we can derive.** Parents come from the litter, the line from the parents, the wean date
   from the birth date, and DPS from the surgery date. Session metadata is read from the TDT block.
4. **Correct, don't overwrite.** Every change keeps history (P1, P11), and undo is a first-class action.
5. **The colony serves the analysis.** A cohort is a saved query that becomes the input to analysis (P7).
6. **Blinding and exclusion are built into the workflow**, not reconstructed at write-up time (P12).
7. **Free and local first.** The lab runs one file, no server, no per-seat price, and the data is readable
   SQLite/CSV forever (P2, P3, P5).
8. **Mac-native:** ⌘K command bar, tables with type-to-select, inspector panes, Quick Look on cards, Spotlight
   indexing of labels. Accessibility labels on every control.

### Concrete flows (measure: taps/keys from idle)
| Flow | How it beats incumbents |
|---|---|
| **Cage move (3 actions)** | Scan the source cage card, choose the animal, scan the destination card. On Mac: ⌘K `mv 142 C-031` ⏎, or drag the animal row onto a cage in the sidebar. A USB/Bluetooth barcode scanner acts as a keyboard, so scanning costs nothing to implement. One transaction closes the old interval and opens the new one, warns on capacity, sex mixing or protocol mismatch, and offers to reprint the card. |
| **Wean wizard** | Open the litter (or scan the breeding cage), press W, enter `4M 3F`. The app proposes labels and splits animals into cages by sex and capacity, with ear-tag fields in tab order. Confirm, and the cards print in one batch. Incumbents need a record per pup. |
| **Keyboard-first census** | Cages are listed in rack order. Type or scan a cage label, Enter confirms the expected count, and a digit corrects it. Mismatches go to a review list. The census is stored as a dated event. Target: 50 cages in 3 minutes. |
| **QR cage cards** | A PDF in fixed formats (3×5 in card, Avery 5160/L7160 labels, 2×1 in thermal) with the label in large type, a QR with the UUID only, line and genotype, DOB, sex counts, protocol #, owner initials, and for experimental cages the surgery date and targets. Cards print through macOS print or PDF, so no driver is needed. A card reprints on any material change and shows a "stale card" badge. |
| **Surgery day** | Choose a batch of animals, then choose a template ("ACh3.0 NAc, bilateral fiber" holds virus lot, coordinates, fiber spec and analgesia plan). One screen per animal holds only the deltas: weights, actual DV, notes. Post-op checks and analgesia doses become due-tasks automatically. "Same as last" is one key. |
| **Session intake** | Drop a block. The app reads the TDT block name and time, guesses the animal from the subject name in Synapse or from the folder, and asks only for paradigm and condition (both preselected from the last session). It writes `session.toml` and computes the digest. A wrong guess takes one keystroke to fix. |
| **Histology pass** | A cohort grid lists every implant. Click an implant to enter the verified tip on an atlas slice picker and set placement and expression flags. The grid is blinded by default. The output is a fiber-tip atlas figure and pre-filled Exclusions. |
| **Cohort → analysis** | Filter with chips (line, genotype, implant region, paradigm, DPS range, not excluded), preview the n per group, then Freeze. Run v0.2 two-stage inference directly on the frozen cohort, with `run.json` carrying the cohort hash. |
| **Animal timeline** | A vertical timeline with filters (husbandry / procedures / sessions / notes). Clicking a session opens its PETH. The animal can be exported as a single PDF "animal record" for an IACUC or vet request. |
| **Import** | Import Excel/CSV from SoftMouse, mLIMS or spreadsheets with column mapping, a **dry-run diff** and atomic commit or rollback (copying Colony.care and JCMS). Exports are always available: CSV per table, the SQLite file itself, and NWB `Subject` + ndx-ophys-devices for sessions. |

### Prioritization
**MVP (v0.5a; one Mac, one lab)**
- Data model: User, Protocol, Line/Allele/GenotypeCall, Animal, Cage, CageAssignment, BreedingPair, Litter,
  Procedure + SurgeryFrame + Injection + Implant + VirusLot, Session, Cohort/GroupAssignment, Exclusion, Note,
  AuditEvent.
- Flows: wean wizard, cage move (⌘K + drag + scanner), QR card PDF, keyboard census, surgery template, session
  intake, animal timeline, cohort freeze → analysis, derived task list, CSV import with dry run, CSV/SQLite export.
- Guards: protocol expiry and count, capacity, one open CageAssignment, unblinding audit.
- Checks: one `test_*` for the CageAssignment interval invariant, one for weaning creating N animals atomically,
  and one for verifying the audit hash chain.

**Next (v0.5b)**
- Histology pass + atlas fiber-tip plot · blinding UI masking · ARRIVE n/exclusion table in the generated methods ·
  pedigree view · Transnetyx CSV genotype import · breeding performance and "replace breeder" alerts ·
  NWB Subject/device export.

**Later**
- iPhone/iPad companion (camera QR scanning, offline, sync) · CloudKit sync with real roles (roadmap) · per-line
  protocol counts and approval workflows · weight and welfare score sheets with humane-endpoint thresholds
  (e.g. ≥20% weight loss) · cage consolidation suggestions (a simple heuristic, not "AI") · breeding planner
  (JAX/UZH colony-size formulas) · facility integration (Mosaic/PyRAT CSV census reconciliation) · RFID.

**Deliberately not building:** per diem billing, animal ordering and facility invoicing. Facility systems own
these (P6), and copying them gives the lab a second source of truth. SimpleFP should reconcile with them, not
replace them.

---

## 5. Open questions
1. **Single-user or multi-user?** Is the target "one lab, one Mac" (SQLite, no auth) or "lab with several Macs"
   from day one? The second pulls sync and real authentication into the MVP.
2. **Facility system:** does the lab's facility already run Mosaic, PyRAT or Topaz? If so, the census-reconciliation
   import moves up in priority.
3. **iRATco:** what was it? I could not find it.
4. **Coordinate conventions:** confirm the lab's sign conventions and DV reference before the fields are frozen.

## Sources (main)
- SoftMouse: https://softmouse.net/ · mLIMS: https://bioinforx.com/web/product_mlims_biotech.php ·
  https://bioinforx.com/web/product_mlims_facility.php
- Mosaic: https://animalcare.ubc.ca/conducting-your-research/managing-your-study-animals-mosaic · https://lar.fsu.edu/mosaic-vivarium/
- JCMS: https://www.jax.org/research-and-faculty/tools/jax-colony-management-system · https://pmc.ncbi.nlm.nih.gov/articles/PMC2844967
- Climb/Instem: https://www.instem.com/climbs-innovation-continues-at-instem/ · https://www.instem.com/solutions/study-management/climb/climb-colony-management/
- PyRAT: https://www.scionics.com/pyrat.html · https://www.ucl.ac.uk/biological-services/pyrat
- Topaz: https://researchroadmap.mssm.edu/reference/systems/topaz-granite/ · tick@lab: https://www.a-tune.com/products-services-software/animal-research-facility-software/
- Labguru: https://help.labguru.com/en/articles/1492340-managing-your-rodent-colonies-in-labguru · LabCollector: https://labcollector.com/labcollector-lims/features/modules/animals-module/
- MausDB: https://pmc.ncbi.nlm.nih.gov/articles/PMC2292142/ · AniBio: https://www.tecniplast.it/en/news/tecniplast-dvc-talks-to-anibio-software-from-noraybio.html
- Transnetyx Colony: https://transnetyx.com/resources/partnerships/ouhsc · https://transnetyx.com/resources/partnerships/cedars-sinai · Labii: https://docs.labii.com/applications/mouse-colony-management
- Moustra: https://moustra.com/blog/softmouse-alternative · Colony.care: https://colony.care/en/compare/colony-care-vs-excel · CageSync: https://www.capterra.com/p/10045702/CageSync/
- MyVivarium: https://pmc.ncbi.nlm.nih.gov/articles/PMC12621272/ · TopoDB: https://pmc.ncbi.nlm.nih.gov/articles/PMC7673335 · RodentSQL: https://pmc.ncbi.nlm.nih.gov/articles/PMC6153456/
- Alyx models: https://github.com/cortex-lab/alyx (subjects/models.py, actions/models.py) · AIND procedures schema: https://aind-data-schema.readthedocs.io/en/latest/components/subject_procedures.html · NWB ndx-ophys-devices: https://ndx-ophys-devices.readthedocs.io/en/stable/description.html · BrainSTEM: https://world-wide.org/fens-26/brainstem-collaborative-electronic-96039b13
- Rutgers QR SOP: https://research.rutgers.edu/system/files/2025-11/husbandry-sop-3-21-qr-scanning-births-overcrowded-etc.pdf · MGH report: https://medinformatics.mgh.harvard.edu/resources/vivarium-colony-management.html · ARRIVE 2.0 summary: https://www.journalmetrics.org/blog/arrive-2-0-animal-research-reporting-guide
