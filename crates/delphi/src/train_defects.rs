//! `delphi train defects`: does a reviewing seat find REAL defects, or only plausible-sounding ones? (`specs/037` §5, M3 of the
//! build plan of 2026-11-07; `specs/048` spirit: no model grades a model.)
//!
//! A seeded generator writes a small synthetic Python module. It holds K **planted defects** of known kinds from a fixed
//! catalogue and N **decoys**: constructs that look suspicious and are correct. The answer key (each defect's line and kind,
//! each decoy's line) goes to a file outside the fixture, so the seat under test never sees it. The seat answers with lines in a
//! fixed format, `FINDING <line> <kind>: <one sentence>`, and scoring is a regular expression and arithmetic:
//!
//! - a finding **catches** a planted defect when its line is within one of the defect's line and its kind is one the key accepts
//!   for that defect (a second finding for a defect already caught is ignored);
//! - a finding within one line of a **decoy** is a **false positive**;
//! - any other finding is **unmatched** (a real line with the wrong kind, or a line that holds nothing).
//!
//! # What this is not
//!
//! These figures (recall, false positives, unmatched, unique catches) are **diagnostics in a training report**. They are not a
//! seat score. They are never an input to routing, ranking, staffing, a Council decision or any default (`specs/078`: Council
//! measurement may not score seat accuracy as a ranking or routing input; Goodhart: a number that picks seats trains seats to
//! the number). The `review_findings` check's pass bit is a threshold on one synthetic case in one experiment, nothing more.
//!
//! Nothing here calls a model.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::train_items::Rng;

/// The kinds a defect may have, in the order the prompt lists them.
pub const KINDS: [&str; 8] = ["off-by-one", "inverted-condition", "missing-error-check", "wrong-operator", "resource-leak", "swapped-arguments", "stale-constant", "injection"];

/// The kinds the subtler templates add (`medium` and `hard`). The prompt lists these too from `medium` up.
pub const HARD_KINDS: [&str; 7] = ["race", "integer-truncation", "swallowed-error", "use-after-close", "unit-mismatch", "contract-violation", "invariant-violation"];

/// How hard a generated case is: how many constructs, which kinds, how long the module. `Easy` is the original case and its
/// bytes do not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    /// The eight original kinds, up to 8 defects and 8 decoys.
    Easy,
    /// Adds races, truncation, swallowed errors, use-after-close and unit mix-ups; up to 16 of each.
    Medium,
    /// Every kind, including a fault behind a helper, a cross-function contract fault and a broken comment invariant; up to 24.
    Hard,
}

impl Difficulty {
    pub fn parse(s: &str) -> Option<Difficulty> {
        match s {
            "easy" => Some(Difficulty::Easy),
            "medium" => Some(Difficulty::Medium),
            "hard" => Some(Difficulty::Hard),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
        }
    }

    /// The most defects (and the most decoys) a case of this difficulty holds.
    pub fn cap(self) -> usize {
        match self {
            Difficulty::Easy => 8,
            Difficulty::Medium => 16,
            Difficulty::Hard => 24,
        }
    }

    /// The counts `gen` and `run` use when `--defects` and `--decoys` are not given.
    pub fn defaults(self) -> (usize, usize) {
        match self {
            Difficulty::Easy => (5, 4),
            Difficulty::Medium => (10, 8),
            Difficulty::Hard => (20, 20),
        }
    }

    /// How many of [`HARD_DEFECTS`] this difficulty draws from.
    fn extra(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Medium => 10,
            Difficulty::Hard => 16,
        }
    }

    fn kinds(self) -> Vec<&'static str> {
        let mut k = KINDS.to_vec();
        if self != Difficulty::Easy {
            k.extend(HARD_KINDS);
        }
        k
    }
}

/// The kinds a finding may name for a defect of `kind`. Neighbours are accepted where two honest names exist for one fault
/// (`value < low and value > high` is a wrong operator and is also a wrong condition); nothing else is.
pub fn accepted(kind: &str) -> &'static [&'static str] {
    match kind {
        "off-by-one" => &["off-by-one", "wrong-operator"],
        "inverted-condition" => &["inverted-condition", "wrong-operator"],
        "missing-error-check" => &["missing-error-check"],
        "wrong-operator" => &["wrong-operator", "inverted-condition"],
        "resource-leak" => &["resource-leak"],
        "swapped-arguments" => &["swapped-arguments"],
        "stale-constant" => &["stale-constant"],
        "injection" => &["injection"],
        "race" => &["race"],
        "integer-truncation" => &["integer-truncation", "wrong-operator"],
        "swallowed-error" => &["swallowed-error", "missing-error-check"],
        "use-after-close" => &["use-after-close", "resource-leak"],
        "unit-mismatch" => &["unit-mismatch", "wrong-operator"],
        "contract-violation" => &["contract-violation"],
        "invariant-violation" => &["invariant-violation", "wrong-operator"],
        _ => &[],
    }
}

/// One construct: its lines, and which of them is the one a review should name. `{f}` is a function name, `{C}` a constant
/// prefix, `{n}` and `{m}` numbers (`{m}` differs from `{n}`).
struct Tpl {
    kind: &'static str,
    /// Decoys only: why the construct is correct.
    why: &'static str,
    target: usize,
    lines: &'static [&'static str],
    /// Another line of the same part that a finding may cite instead (the act of a check-then-act, the use behind a helper).
    also: Option<usize>,
    /// A cross-function construct: this second part is placed far from the first. Its target is the planted line (correct on its
    /// own, wrong given what the first part promises); `also` then points into the FIRST part (the promise).
    second: Option<(&'static [&'static str], usize)>,
}

const DEFECTS: [Tpl; 8] = [
    Tpl { kind: "off-by-one", why: "", target: 3, lines: &["def {f}(rows):", "    \"\"\"Return the sum of every value in rows.\"\"\"", "    total = 0", "    for i in range(1, len(rows)):", "        total += rows[i]", "    return total"], also: None, second: None },
    Tpl { kind: "inverted-condition", why: "", target: 2, lines: &["def {f}(balance, amount):", "    \"\"\"Withdraw amount; refuse when the balance does not cover it.\"\"\"", "    if balance >= amount:", "        raise ValueError(\"insufficient funds\")", "    return balance - amount"], also: None, second: None },
    Tpl { kind: "missing-error-check", why: "", target: 3, lines: &["def {f}(conn, key):", "    \"\"\"Return the cached value for key; raise KeyError when it is absent.\"\"\"", "    row = conn.execute(\"SELECT value FROM cache WHERE key = ?\", (key,)).fetchone()", "    return row[0]"], also: None, second: None },
    Tpl { kind: "wrong-operator", why: "", target: 2, lines: &["def {f}(value, low, high):", "    \"\"\"Return True when value lies outside the range low to high.\"\"\"", "    return value < low and value > high"], also: None, second: None },
    Tpl { kind: "resource-leak", why: "", target: 2, lines: &["def {f}(path):", "    \"\"\"Return the number of lines in the file at path.\"\"\"", "    fh = open(path)", "    count = sum(1 for _ in fh)", "    return count"], also: None, second: None },
    Tpl { kind: "swapped-arguments", why: "", target: 2, lines: &["def {f}(src, dst):", "    \"\"\"Copy the file at src to dst.\"\"\"", "    shutil.copyfile(dst, src)"], also: None, second: None },
    Tpl { kind: "stale-constant", why: "", target: 5, lines: &["{C}_PAGE_SIZE = {n}", "", "", "def {f}(items, page):", "    \"\"\"Return the items on a page of {C}_PAGE_SIZE items; pages count from 0.\"\"\"", "    start = page * {m}", "    return items[start:start + {C}_PAGE_SIZE]"], also: None, second: None },
    Tpl { kind: "injection", why: "", target: 2, lines: &["def {f}(conn, name):", "    \"\"\"Return the row of the user called name.\"\"\"", "    query = \"SELECT * FROM users WHERE name = '\" + name + \"'\"", "    return conn.execute(query).fetchone()"], also: None, second: None },
];

/// Constructs that look like the defects above and are correct. Indexed like [`DEFECTS`] by the fault they imitate.
const DECOYS: [Tpl; 8] = [
    Tpl { kind: "off-by-one", why: "pairs each value with its predecessor, so starting at 1 is right", target: 3, lines: &["def {f}(rows):", "    \"\"\"Return the differences between consecutive values.\"\"\"", "    out = []", "    for i in range(1, len(rows)):", "        out.append(rows[i] - rows[i - 1])", "    return out"], also: None, second: None },
    Tpl { kind: "inverted-condition", why: "the negated range test rejects exactly the values outside 0 to 100", target: 2, lines: &["def {f}(pct):", "    \"\"\"Reject a percentage outside 0 to 100.\"\"\"", "    if not 0 <= pct <= 100:", "        raise ValueError(\"percentage out of range\")", "    return pct / 100"], also: None, second: None },
    Tpl { kind: "missing-error-check", why: "the None case is handled two lines above", target: 5, lines: &["def {f}(conn, key):", "    \"\"\"Return the cached value for key; raise KeyError when it is absent.\"\"\"", "    row = conn.execute(\"SELECT value FROM cache WHERE key = ?\", (key,)).fetchone()", "    if row is None:", "        raise KeyError(key)", "    return row[0]"], also: None, second: None },
    Tpl { kind: "wrong-operator", why: "the negated chained comparison is the outside-the-range test", target: 2, lines: &["def {f}(value, low, high):", "    \"\"\"Return True when value lies outside the range low to high.\"\"\"", "    return not (low <= value <= high)"], also: None, second: None },
    Tpl { kind: "resource-leak", why: "the file is closed in the finally block", target: 2, lines: &["def {f}(path):", "    \"\"\"Return the number of lines in the file at path.\"\"\"", "    fh = open(path)", "    try:", "        return sum(1 for _ in fh)", "    finally:", "        fh.close()"], also: None, second: None },
    Tpl { kind: "swapped-arguments", why: "relpath takes the target first and the start second, as written", target: 2, lines: &["def {f}(frm, to):", "    \"\"\"Return the path of `to` relative to the directory `frm`.\"\"\"", "    return os.path.relpath(to, frm)"], also: None, second: None },
    Tpl { kind: "stale-constant", why: "the literal is the documented page size", target: 2, lines: &["def {f}(items, page):", "    \"\"\"Return the items on a page of {n} items; pages count from 0.\"\"\"", "    start = page * {n}", "    return items[start:start + {n}]"], also: None, second: None },
    Tpl { kind: "injection", why: "the table name comes from a fixed mapping; the value is a bound parameter", target: 6, lines: &["{C}_TABLES = {\"orders\": \"orders_2024\", \"refunds\": \"refunds_2024\"}", "", "", "def {f}(conn, kind, ident):", "    \"\"\"Return the row with id ident from the table for kind (one of {C}_TABLES).\"\"\"", "    table = {C}_TABLES[kind]", "    return conn.execute(f\"SELECT * FROM {table} WHERE id = ?\", (ident,)).fetchone()"], also: None, second: None },
];

/// Subtler defects, used from `medium` up (the first ten) and `hard` (all sixteen). Two of each kind, in this order: race,
/// integer-truncation, swallowed-error, use-after-close, unit-mismatch, off-by-one behind a helper, cross-function contract,
/// invariant stated in a comment. `HARD_DECOYS` below is the same list, one correct twin per entry.
const HARD_DEFECTS: [Tpl; 16] = [
    Tpl { kind: "race", why: "", target: 2, lines: &[r#"def {f}(path, data):"#, r#"    """Create the file at path with data; never overwrite an existing file.""""#, r#"    if not os.path.exists(path):"#, r#"        with open(path, "w") as fh:"#, r#"            fh.write(data)"#], also: Some(3), second: None },
    Tpl { kind: "race", why: "", target: 3, lines: &[r#"def {f}(lock_dir, name):"#, r#"    """Take the named lock; return False when someone else already holds it.""""#, r#"    path = os.path.join(lock_dir, name)"#, r#"    if os.path.isdir(path):"#, r#"        return False"#, r#"    os.mkdir(path)"#, r#"    return True"#], also: Some(5), second: None },
    Tpl { kind: "integer-truncation", why: "", target: 2, lines: &[r#"def {f}(total_bytes, chunk):"#, r#"    """Return the number of chunks needed for total_bytes; a partial chunk counts as one.""""#, r#"    return int(total_bytes / chunk)"#], also: None, second: None },
    Tpl { kind: "integer-truncation", why: "", target: 5, lines: &[r#"def {f}(counts):"#, r#"    """Return the sum of counts as a signed 16-bit value; raise OverflowError when it does not fit.""""#, r#"    total = sum(counts)"#, r#"    if total < 0:"#, r#"        raise OverflowError(total)"#, r#"    return struct.unpack("<h", struct.pack("<H", total & 0xFFFF))[0]"#], also: None, second: None },
    Tpl { kind: "swallowed-error", why: "", target: 5, lines: &[r#"def {f}(path):"#, r#"    """Return the parsed JSON in the file at path; raise when it cannot be read or parsed.""""#, r#"    try:"#, r#"        with open(path) as fh:"#, r#"            return json.load(fh)"#, r#"    except Exception:"#, r#"        return {}"#], also: None, second: None },
    Tpl { kind: "swallowed-error", why: "", target: 5, lines: &[r#"def {f}(conn, rows):"#, r#"    """Insert rows; when one insert fails the error must reach the caller.""""#, r#"    for row in rows:"#, r#"        try:"#, r#"            conn.execute("INSERT INTO log VALUES (?)", (row,))"#, r#"        except sqlite3.Error:"#, r#"            pass"#, r#"    conn.commit()"#], also: None, second: None },
    Tpl { kind: "use-after-close", why: "", target: 4, lines: &[r#"def {f}(path):"#, r#"    """Return the first row of the CSV file at path.""""#, r#"    with open(path, newline="") as fh:"#, r#"        reader = csv.reader(fh)"#, r#"    return next(reader)"#], also: None, second: None },
    Tpl { kind: "use-after-close", why: "", target: 4, lines: &[r#"def {f}(conn, ids):"#, r#"    """Return the names for ids, then close the connection.""""#, r#"    try:"#, r#"        names = (conn.execute("SELECT name FROM t WHERE id = ?", (i,)).fetchone()[0] for i in ids)"#, r#"        return names"#, r#"    finally:"#, r#"        conn.close()"#], also: None, second: None },
    Tpl { kind: "unit-mismatch", why: "", target: 2, lines: &[r#"def {f}(deadline_ms):"#, r#"    """Sleep until the deadline, given in milliseconds since the epoch.""""#, r#"    time.sleep(deadline_ms - time.time())"#], also: None, second: None },
    Tpl { kind: "unit-mismatch", why: "", target: 2, lines: &[r#"def {f}(event, timeout_ms):"#, r#"    """Wait for the event for at most timeout_ms milliseconds; True when it was set.""""#, r#"    return event.wait(timeout_ms)"#], also: None, second: None },
    Tpl { kind: "off-by-one", why: "", target: 2, lines: &[r#"def {f}_last(rows):"#, r#"    """Return the index of the last row.""""#, r#"    return len(rows)"#, r#""#, r#""#, r#"def {f}(rows):"#, r#"    """Return the last row.""""#, r#"    return rows[{f}_last(rows)]"#], also: Some(7), second: None },
    Tpl { kind: "off-by-one", why: "", target: 8, lines: &[r#"def {f}_limit(n):"#, r#"    """Return the largest index to visit when scanning n items.""""#, r#"    return n - 1"#, r#""#, r#""#, r#"def {f}(items):"#, r#"    """Return the sum of every item.""""#, r#"    total = 0"#, r#"    for i in range({f}_limit(len(items))):"#, r#"        total += items[i]"#, r#"    return total"#], also: Some(2), second: None },
    Tpl { kind: "contract-violation", why: "", target: 3, lines: &[r#"def {f}_raw(conn, key):"#, r#"    """Return the stored value for key, or None when key is absent.""""#, r#"    row = conn.execute("SELECT value FROM cache WHERE key = ?", (key,)).fetchone()"#, r#"    return row[0] if row else None"#], also: Some(3), second: Some((&[r#"def {f}(conn, key):"#, r#"    """Return the length of the stored value for key.""""#, r#"    return len({f}_raw(conn, key))"#], 2)) },
    Tpl { kind: "contract-violation", why: "", target: 2, lines: &[r#"def {f}_ids(conn):"#, r#"    """Return the ids of all rows, newest first.""""#, r#"    return [r[0] for r in conn.execute("SELECT id FROM t ORDER BY created DESC")]"#], also: Some(2), second: Some((&[r#"def {f}(conn):"#, r#"    """Return the id of the oldest row.""""#, r#"    return {f}_ids(conn)[0]"#], 2)) },
    Tpl { kind: "invariant-violation", why: "", target: 5, lines: &[r#"def {f}(window, value):"#, r#"    """Append value to window, which holds at most 8 values.""""#, r#"    # Invariant: len(window) <= 8 on entry and on exit; the oldest value is dropped first."#, r#"    window.append(value)"#, r#"    if len(window) > 8:"#, r#"        window.pop()"#, r#"    return window"#], also: None, second: None },
    Tpl { kind: "invariant-violation", why: "", target: 5, lines: &[r#"def {f}(stack, item):"#, r#"    """Push item onto stack, which is kept in ascending order.""""#, r#"    # Invariant: stack is ascending, so stack[-1] is the largest value."#, r#"    if stack and item < stack[-1]:"#, r#"        raise ValueError("item would break the ordering")"#, r#"    stack.insert(0, item)"#, r#"    return stack"#], also: None, second: None },
];

/// One correct twin for each of [`HARD_DEFECTS`], same index: the same shape, with the guard, the documentation or the intent
/// that makes it right.
const HARD_DECOYS: [Tpl; 16] = [
    Tpl { kind: "race", why: "the check and the create happen under a lock the caller holds", target: 3, lines: &[r#"def {f}(path, data, lock):"#, r#"    """Create the file at path with data; never overwrite an existing file.""""#, r#"    with lock:"#, r#"        if not os.path.exists(path):"#, r#"            with open(path, "w") as fh:"#, r#"                fh.write(data)"#], also: Some(4), second: None },
    Tpl { kind: "race", why: "mkdir is the atomic step and its failure is the answer; there is no separate check", target: 3, lines: &[r#"def {f}(lock_dir, name):"#, r#"    """Take the named lock; return False when someone else already holds it.""""#, r#"    path = os.path.join(lock_dir, name)"#, r#"    try:"#, r#"        os.mkdir(path)"#, r#"    except FileExistsError:"#, r#"        return False"#, r#"    return True"#], also: Some(4), second: None },
    Tpl { kind: "integer-truncation", why: "negated floor division is ceiling division, which is what the docstring asks for", target: 2, lines: &[r#"def {f}(total_bytes, chunk):"#, r#"    """Return the number of chunks needed for total_bytes; a partial chunk counts as one.""""#, r#"    return -(-total_bytes // chunk)"#], also: None, second: None },
    Tpl { kind: "integer-truncation", why: "the range is checked on both sides before the wrap, so nothing is lost", target: 5, lines: &[r#"def {f}(counts):"#, r#"    """Return the sum of counts as a signed 16-bit value; raise OverflowError when it does not fit.""""#, r#"    total = sum(counts)"#, r#"    if not -32768 <= total <= 32767:"#, r#"        raise OverflowError(total)"#, r#"    return struct.unpack("<h", struct.pack("<H", total & 0xFFFF))[0]"#], also: None, second: None },
    Tpl { kind: "swallowed-error", why: "the docstring says a missing file reads as empty; any other error still propagates", target: 5, lines: &[r#"def {f}(path):"#, r#"    """Return the parsed JSON in the file at path, or {} when the file does not exist.""""#, r#"    try:"#, r#"        with open(path) as fh:"#, r#"            return json.load(fh)"#, r#"    except FileNotFoundError:"#, r#"        return {}"#], also: None, second: None },
    Tpl { kind: "swallowed-error", why: "the handler rolls back and re-raises, so the caller sees the error", target: 5, lines: &[r#"def {f}(conn, rows):"#, r#"    """Insert rows; when one insert fails the error must reach the caller.""""#, r#"    for row in rows:"#, r#"        try:"#, r#"            conn.execute("INSERT INTO log VALUES (?)", (row,))"#, r#"        except sqlite3.Error:"#, r#"            conn.rollback()"#, r#"            raise"#, r#"    conn.commit()"#], also: None, second: None },
    Tpl { kind: "use-after-close", why: "the row is read while the file is still open", target: 4, lines: &[r#"def {f}(path):"#, r#"    """Return the first row of the CSV file at path.""""#, r#"    with open(path, newline="") as fh:"#, r#"        reader = csv.reader(fh)"#, r#"        return next(reader)"#], also: None, second: None },
    Tpl { kind: "use-after-close", why: "list() drains the generator before the finally block closes the connection", target: 4, lines: &[r#"def {f}(conn, ids):"#, r#"    """Return the names for ids, then close the connection.""""#, r#"    try:"#, r#"        names = (conn.execute("SELECT name FROM t WHERE id = ?", (i,)).fetchone()[0] for i in ids)"#, r#"        return list(names)"#, r#"    finally:"#, r#"        conn.close()"#], also: None, second: None },
    Tpl { kind: "unit-mismatch", why: "the deadline is converted from milliseconds to seconds before it meets time.time()", target: 2, lines: &[r#"def {f}(deadline_ms):"#, r#"    """Sleep until the deadline, given in milliseconds since the epoch.""""#, r#"    time.sleep(max(0.0, deadline_ms / 1000.0 - time.time()))"#], also: None, second: None },
    Tpl { kind: "unit-mismatch", why: "Event.wait takes seconds and the milliseconds are divided down", target: 2, lines: &[r#"def {f}(event, timeout_ms):"#, r#"    """Wait for the event for at most timeout_ms milliseconds; True when it was set.""""#, r#"    return event.wait(timeout_ms / 1000)"#], also: None, second: None },
    Tpl { kind: "off-by-one", why: "the helper returns a slice end, one past the last index, and is only used as a slice end", target: 2, lines: &[r#"def {f}_end(rows):"#, r#"    """Return the slice end that covers every row.""""#, r#"    return len(rows)"#, r#""#, r#""#, r#"def {f}(rows):"#, r#"    """Return a copy of every row.""""#, r#"    return rows[:{f}_end(rows)]"#], also: Some(7), second: None },
    Tpl { kind: "off-by-one", why: "the helper is the last index and the loop adds one to make it a range end", target: 8, lines: &[r#"def {f}_limit(n):"#, r#"    """Return the largest index to visit when scanning n items.""""#, r#"    return n - 1"#, r#""#, r#""#, r#"def {f}(items):"#, r#"    """Return the sum of every item.""""#, r#"    total = 0"#, r#"    for i in range({f}_limit(len(items)) + 1):"#, r#"        total += items[i]"#, r#"    return total"#], also: Some(2), second: None },
    Tpl { kind: "contract-violation", why: "the caller handles the None the callee promises for an absent key", target: 3, lines: &[r#"def {f}_raw(conn, key):"#, r#"    """Return the stored value for key, or None when key is absent.""""#, r#"    row = conn.execute("SELECT value FROM cache WHERE key = ?", (key,)).fetchone()"#, r#"    return row[0] if row else None"#], also: Some(3), second: Some((&[r#"def {f}(conn, key):"#, r#"    """Return the length of the stored value for key; 0 when it is absent.""""#, r#"    value = {f}_raw(conn, key)"#, r#"    return 0 if value is None else len(value)"#], 3)) },
    Tpl { kind: "contract-violation", why: "the caller takes the last element of a newest-first list, which is the oldest row", target: 2, lines: &[r#"def {f}_ids(conn):"#, r#"    """Return the ids of all rows, newest first.""""#, r#"    return [r[0] for r in conn.execute("SELECT id FROM t ORDER BY created DESC")]"#], also: Some(2), second: Some((&[r#"def {f}(conn):"#, r#"    """Return the id of the oldest row.""""#, r#"    return {f}_ids(conn)[-1]"#], 2)) },
    Tpl { kind: "invariant-violation", why: "pop(0) drops the oldest value, as the invariant says", target: 5, lines: &[r#"def {f}(window, value):"#, r#"    """Append value to window, which holds at most 8 values.""""#, r#"    # Invariant: len(window) <= 8 on entry and on exit; the oldest value is dropped first."#, r#"    window.append(value)"#, r#"    if len(window) > 8:"#, r#"        window.pop(0)"#, r#"    return window"#], also: None, second: None },
    Tpl { kind: "invariant-violation", why: "the new value is the largest, so appending keeps the order ascending", target: 5, lines: &[r#"def {f}(stack, item):"#, r#"    """Push item onto stack, which is kept in ascending order.""""#, r#"    # Invariant: stack is ascending, so stack[-1] is the largest value."#, r#"    if stack and item < stack[-1]:"#, r#"        raise ValueError("item would break the ordering")"#, r#"    stack.append(item)"#, r#"    return stack"#], also: None, second: None },
];

const VERBS: [&str; 8] = ["load", "fetch", "build", "check", "apply", "merge", "read", "count"];
const NOUNS: [&str; 8] = ["order", "ledger", "invoice", "session", "token", "quota", "report", "batch"];
const HEADER: [&str; 4] = ["\"\"\"Order and account helpers.\"\"\"", "import os", "import shutil", ""];
const HEADER_HARD: [&str; 9] = ["\"\"\"Order and account helpers.\"\"\"", "import csv", "import json", "import os", "import shutil", "import sqlite3", "import struct", "import time", ""];

#[derive(Debug, Clone, PartialEq)]
pub struct Defect {
    pub line: usize,
    pub kind: String,
    /// A second line a finding may cite instead (within one line of it counts): the act of a check-then-act, or, for a
    /// cross-function defect, the callee line whose promise the planted line breaks.
    pub also: Option<usize>,
}

impl Defect {
    /// A stable name within a case: the line and the kind. Unique catches are counted by this and the case id.
    pub fn id(&self) -> String {
        format!("{}:{}", self.line, self.kind)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decoy {
    pub line: usize,
    pub imitates: String,
    pub why: String,
    pub also: Option<usize>,
}

/// The answer key. It is written outside the fixture and read only by the check.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub defects: Vec<Defect>,
    pub decoys: Vec<Decoy>,
}

pub struct Artifact {
    pub source: String,
    pub key: Key,
}

fn shuffle<T>(rng: &mut Rng, xs: &mut [T]) {
    for i in (1..xs.len()).rev() {
        xs.swap(i, rng.below(i + 1));
    }
}

/// The easy module for `seed` with `k` planted defects (1 to 8, distinct kinds) and `decoys` correct look-alikes (0 to 8,
/// distinct). The same arguments give the same bytes on every platform.
#[cfg(test)]
pub fn generate(seed: u64, k: usize, decoys: usize) -> Artifact {
    generate_at(seed, k, decoys, Difficulty::Easy)
}

/// What a part of the file is: a whole construct, or one half of a cross-function pair.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    Whole,
    Callee,
    Caller,
}

/// The module for `seed` at `difficulty` with `k` planted defects and `decoys` correct look-alikes, each clamped to what the
/// difficulty allows. Medium and hard repeat kinds (two templates per subtle kind) and place the two halves of a cross-function
/// construct far apart. The same arguments give the same bytes on every platform.
pub fn generate_at(seed: u64, k: usize, decoys: usize, difficulty: Difficulty) -> Artifact {
    let mut rng = Rng::new(seed ^ 0x00D3_FEC7);
    let pool = 8 + difficulty.extra();
    let tpl_d = |i: usize| if i < 8 { &DEFECTS[i] } else { &HARD_DEFECTS[i - 8] };
    let tpl_c = |i: usize| if i < 8 { &DECOYS[i] } else { &HARD_DECOYS[i - 8] };
    let (k, decoys) = (k.clamp(1, difficulty.cap().min(pool)), decoys.min(difficulty.cap().min(pool)));
    let mut d_idx: Vec<usize> = (0..pool).collect();
    shuffle(&mut rng, &mut d_idx);
    let mut c_idx: Vec<usize> = (0..pool).collect();
    shuffle(&mut rng, &mut c_idx);
    // Entries in a fixed order (defects, then decoys); parts are what lands in the file.
    let chosen: Vec<(&Tpl, bool)> = d_idx[..k].iter().map(|i| (tpl_d(*i), true)).chain(c_idx[..decoys].iter().map(|i| (tpl_c(*i), false))).collect();
    let mut parts: Vec<(usize, Role)> = (0..chosen.len()).filter(|i| chosen[*i].0.second.is_none()).map(|i| (i, Role::Whole)).collect();
    let cross: Vec<usize> = (0..chosen.len()).filter(|i| chosen[*i].0.second.is_some()).collect();
    shuffle(&mut rng, &mut parts);
    let mut names: Vec<(usize, usize)> = (0..64).map(|i| (i / 8, i % 8)).collect();
    shuffle(&mut rng, &mut names);
    // A name per entry: whole constructs by their place in the file (as the easy case always did), cross pairs after them.
    let mut name_of: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for (pos, (e, _)) in parts.iter().enumerate() {
        name_of.insert(*e, names[pos]);
    }
    for (j, e) in cross.iter().enumerate() {
        name_of.insert(*e, names[parts.len() + j]);
    }
    // The callee goes in the first third of the file and the caller in the last third, so the two are read together only by a
    // reader who holds both in mind.
    for e in &cross {
        let n = parts.len();
        parts.insert(rng.below(n / 3 + 1), (*e, Role::Callee));
        let (n, lo) = (parts.len(), 2 * parts.len() / 3);
        parts.insert(lo + rng.below(n - lo + 1), (*e, Role::Caller));
    }
    let header: &[&str] = if difficulty == Difficulty::Easy { &HEADER } else { &HEADER_HARD };
    let mut lines: Vec<String> = header.iter().map(|s| s.to_string()).collect();
    // Per entry: the planted or imitated line, and the second line.
    let mut primary: BTreeMap<usize, usize> = BTreeMap::new();
    let mut secondary: BTreeMap<usize, usize> = BTreeMap::new();
    for (e, role) in &parts {
        let (t, _) = chosen[*e];
        let (v, n) = name_of[e];
        let num = [20usize, 25, 40, 50, 100][rng.below(5)];
        let sub = |s: &str| s.replace("{f}", &format!("{}_{}", VERBS[v], NOUNS[n])).replace("{C}", &NOUNS[n].to_uppercase()).replace("{n}", &num.to_string()).replace("{m}", &(num * 2).to_string());
        lines.push(String::new());
        lines.push(String::new());
        // Header lines count too: `at` was computed from the length before the block, so it is already the 1-based file line.
        let at = lines.len() + 1;
        match role {
            Role::Whole => {
                lines.extend(t.lines.iter().map(|l| sub(l)));
                primary.insert(*e, at + t.target);
                if let Some(a) = t.also {
                    secondary.insert(*e, at + a);
                }
            }
            Role::Callee => {
                lines.extend(t.lines.iter().map(|l| sub(l)));
                secondary.insert(*e, at + t.also.unwrap_or(t.target));
            }
            Role::Caller => {
                let (body, target) = t.second.expect("a cross template has a second part");
                lines.extend(body.iter().map(|l| sub(l)));
                primary.insert(*e, at + target);
            }
        }
    }
    let (mut defects, mut decoy_rows) = (Vec::new(), Vec::new());
    for (e, (t, is_defect)) in chosen.iter().enumerate() {
        let (line, also) = (primary[&e], secondary.get(&e).copied());
        if *is_defect {
            defects.push(Defect { line, kind: t.kind.to_string(), also });
        } else {
            decoy_rows.push(Decoy { line, imitates: t.kind.to_string(), why: t.why.to_string(), also });
        }
    }
    defects.sort_by_key(|d| d.line);
    decoy_rows.sort_by_key(|d| d.line);
    let mut source = lines.join("\n");
    source.push('\n');
    Artifact { source, key: Key { defects, decoys: decoy_rows } }
}

impl Key {
    pub fn to_json(&self, seed: u64, file: &str) -> Value {
        json!({"seed": seed, "file": file,
               "defects": self.defects.iter().map(|d| match d.also {
                   Some(a) => json!({"id": d.id(), "line": d.line, "also_line": a, "kind": d.kind, "accept": accepted(&d.kind)}),
                   None => json!({"id": d.id(), "line": d.line, "kind": d.kind, "accept": accepted(&d.kind)}),
               }).collect::<Vec<_>>(),
               "decoys": self.decoys.iter().map(|d| match d.also {
                   Some(a) => json!({"line": d.line, "also_line": a, "imitates": d.imitates, "why": d.why}),
                   None => json!({"line": d.line, "imitates": d.imitates, "why": d.why}),
               }).collect::<Vec<_>>()})
    }

    pub fn from_json(text: &str) -> Result<Key, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON ({})", e))?;
        let defects: Vec<Defect> = v["defects"].as_array().ok_or("no `defects` array")?.iter().filter_map(|d| Some(Defect { line: d["line"].as_u64()? as usize, kind: d["kind"].as_str()?.to_string(), also: d["also_line"].as_u64().map(|a| a as usize) })).collect();
        if defects.is_empty() {
            return Err("the key lists no defects".into());
        }
        let decoys = v["decoys"].as_array().map(|a| a.iter().filter_map(|d| Some(Decoy { line: d["line"].as_u64()? as usize, imitates: d["imitates"].as_str().unwrap_or("").to_string(), why: d["why"].as_str().unwrap_or("").to_string(), also: d["also_line"].as_u64().map(|a| a as usize) })).collect()).unwrap_or_default();
        Ok(Key { defects, decoys })
    }
}

/// The findings in an answer: `FINDING <line> <kind>: <sentence>`, one per line, an optional list marker allowed. Anything else
/// in the answer is ignored. Returns `(line, kind)` in order.
pub fn parse_findings(answer: &str) -> Vec<(usize, String)> {
    let re = regex::Regex::new(r"(?im)^[ \t]*(?:[-*][ \t]+)?FINDING[ \t]+(\d+)[ \t]+([a-z][a-z-]*)[ \t]*:").expect("a fixed pattern");
    re.captures_iter(answer).filter_map(|c| Some((c[1].parse::<usize>().ok()?, c[2].to_lowercase()))).collect()
}

/// One answer's figures. Diagnostics only (see the module note).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Score {
    pub findings: usize,
    pub defects: usize,
    /// The ids of the planted defects the answer caught.
    pub caught: Vec<String>,
    pub false_positives: usize,
    pub unmatched: usize,
    /// How many planted defects of each kind the case held (the denominator of per-kind recall; a caught id ends in its kind).
    pub kinds: BTreeMap<String, usize>,
}

impl Score {
    pub fn recall(&self) -> f64 {
        if self.defects == 0 { 0.0 } else { self.caught.len() as f64 / self.defects as f64 }
    }

    pub fn note(&self) -> Value {
        json!({"defects": self.defects, "caught": self.caught, "false_positives": self.false_positives, "unmatched": self.unmatched, "findings": self.findings, "kinds": self.kinds})
    }

    pub fn from_note(v: &Value) -> Option<Score> {
        Some(Score {
            findings: v["findings"].as_u64()? as usize,
            defects: v["defects"].as_u64()? as usize,
            caught: v["caught"].as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            false_positives: v["false_positives"].as_u64()? as usize,
            unmatched: v["unmatched"].as_u64()? as usize,
            kinds: v["kinds"].as_object().map(|o| o.iter().filter_map(|(k, n)| Some((k.clone(), n.as_u64()? as usize))).collect()).unwrap_or_default(),
        })
    }
}

fn near(a: usize, b: usize) -> bool {
    a.abs_diff(b) <= 1
}

/// A finding at `line` is on a recorded construct when it is within one line of either of its lines.
fn on(line: usize, primary: usize, also: Option<usize>) -> bool {
    near(primary, line) || also.is_some_and(|a| near(a, line))
}

pub fn score(key: &Key, answer: &str) -> Score {
    let found = parse_findings(answer);
    let mut s = Score { findings: found.len(), defects: key.defects.len(), ..Score::default() };
    for d in &key.defects {
        *s.kinds.entry(d.kind.clone()).or_default() += 1;
    }
    let mut caught: BTreeSet<String> = BTreeSet::new();
    for (line, kind) in &found {
        if let Some(d) = key.defects.iter().find(|d| on(*line, d.line, d.also) && accepted(&d.kind).contains(&kind.as_str())) {
            caught.insert(d.id());
        } else if key.decoys.iter().any(|d| on(*line, d.line, d.also)) {
            s.false_positives += 1;
        } else {
            s.unmatched += 1;
        }
    }
    s.caught = caught.into_iter().collect();
    s
}

/// The check's reading of a case: the key file in the case directory against the deliverable.
pub fn grade(case_dir: &Path, file: &str, deliverable: &str) -> Result<Score, String> {
    let text = fs::read_to_string(case_dir.join(file)).map_err(|e| format!("{}: {}", file, e))?;
    let key = Key::from_json(&text).map_err(|e| format!("{}: {}", file, e))?;
    Ok(score(&key, deliverable))
}

/// `Ok` when the answer clears the case's thresholds, else what fell short, in words.
pub fn verdict(s: &Score, min_recall: f64, max_false_positives: u64) -> Result<(), String> {
    if s.findings == 0 {
        return Err("no `FINDING <line> <kind>: <sentence>` lines in the deliverable".into());
    }
    let mut why = Vec::new();
    if s.recall() + 1e-9 < min_recall {
        why.push(format!("caught {} of {} planted defects ({:.0}%), wanted {:.0}%", s.caught.len(), s.defects, s.recall() * 100.0, min_recall * 100.0));
    }
    if s.false_positives as u64 > max_false_positives {
        why.push(format!("{} decoy(s) flagged, at most {} allowed", s.false_positives, max_false_positives));
    }
    if why.is_empty() { Ok(()) } else { Err(why.join("; ")) }
}

// ── a scripted instrument check ─────────────────────────────────────────────────────────────────────────────────

/// Scripted reviewers: functions, not models. They show what the scoring can tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reviewer {
    /// Names every planted defect with its true kind and nothing else.
    Thorough,
    /// Flags everything that looks odd: every defect and every decoy.
    Suspicious,
    /// Names only look-alikes and invents a plausible fault on a line that holds nothing wrong.
    Plausible,
    /// Reports the right lines under the wrong kinds.
    Mislabelling,
    /// Says nothing in the format.
    Silent,
}

impl Reviewer {
    pub const ALL: [Reviewer; 5] = [Reviewer::Thorough, Reviewer::Suspicious, Reviewer::Plausible, Reviewer::Mislabelling, Reviewer::Silent];

    pub fn name(self) -> &'static str {
        match self {
            Reviewer::Thorough => "thorough",
            Reviewer::Suspicious => "suspicious",
            Reviewer::Plausible => "plausible",
            Reviewer::Mislabelling => "mislabelling",
            Reviewer::Silent => "silent",
        }
    }

    pub fn answer(self, key: &Key) -> String {
        let line = |l: usize, k: &str| format!("FINDING {} {}: scripted finding.\n", l, k);
        match self {
            Reviewer::Thorough => key.defects.iter().map(|d| line(d.line, &d.kind)).collect(),
            Reviewer::Suspicious => key.defects.iter().map(|d| line(d.line, &d.kind)).chain(key.decoys.iter().map(|d| line(d.line, &d.imitates))).collect(),
            Reviewer::Plausible => key.decoys.iter().map(|d| line(d.line, &d.imitates)).chain(std::iter::once(line(1, "injection"))).collect(),
            Reviewer::Mislabelling => key.defects.iter().map(|d| line(d.line, if d.kind == "injection" { "resource-leak" } else { "injection" })).collect(),
            Reviewer::Silent => "The code looks fine to me.\n".to_string(),
        }
    }
}

/// The table `run` prints.
pub fn table(seed: u64, key: &Key, rows: &[(Reviewer, Score)]) -> String {
    let mut out = format!("SEEDED-DEFECT REVIEW (seed {}, {} planted defects, {} decoys): scripted reviewers\n", seed, key.defects.len(), key.decoys.len());
    out.push_str(&format!("{:<14}{:>10}{:>16}{:>11}\n", "reviewer", "recall", "false positives", "unmatched"));
    for (r, s) in rows {
        out.push_str(&format!("{:<14}{:>10}{:>16}{:>11}\n", r.name(), format!("{}/{}", s.caught.len(), s.defects), s.false_positives, s.unmatched));
    }
    out.push_str(DISCLAIMER);
    out.push('\n');
    out
}

/// Printed under every figure this module produces.
pub const DISCLAIMER: &str = "Diagnostics for a training report, not a seat score: nothing here feeds routing, ranking, staffing or any Council decision (specs/078). Scripted reviewers are functions, not models.";

// ── the report section ──────────────────────────────────────────────────────────────────────────────────────────

/// One graded run's review figures, with the case and arm it belongs to.
pub struct RunReview<'a> {
    pub case: &'a str,
    pub arm: &'a str,
    pub score: &'a Score,
}

/// The `SEEDED-DEFECT REVIEW` section of `delphi train report`: recall, false positives and unique catches by arm. Empty when no
/// run carries review figures.
pub fn report_section(runs: &[RunReview]) -> String {
    if runs.is_empty() {
        return String::new();
    }
    let arms: BTreeSet<&str> = runs.iter().map(|r| r.arm).collect();
    // Per case and arm: the defects any run of that arm caught. A defect is a unique catch when exactly one arm caught it and
    // at least two arms ran the case.
    let mut caught: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    let mut ran: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for r in runs {
        ran.entry(r.case).or_default().insert(r.arm);
        caught.entry((r.case, r.arm)).or_default().extend(r.score.caught.iter().map(String::as_str));
    }
    let mut unique: BTreeMap<&str, usize> = BTreeMap::new();
    for (case, with) in &ran {
        if with.len() < 2 {
            continue;
        }
        let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for arm in with {
            for d in caught.get(&(*case, *arm)).into_iter().flatten() {
                owners.entry(d).or_default().push(arm);
            }
        }
        for (_, o) in owners.iter().filter(|(_, o)| o.len() == 1) {
            *unique.entry(o[0]).or_default() += 1;
        }
    }
    let mut out = String::from("\nSEEDED-DEFECT REVIEW (planted defects found, by arm; diagnostics only)\n");
    out.push_str(&format!("{:<22}{:>6}{:>9}{:>9}{:>12}{:>12}{:>9}\n", "arm", "runs", "recall", "caught", "false pos.", "unmatched", "unique"));
    for arm in &arms {
        let rs: Vec<&RunReview> = runs.iter().filter(|r| r.arm == *arm).collect();
        let (def, hit): (usize, usize) = rs.iter().fold((0, 0), |(d, h), r| (d + r.score.defects, h + r.score.caught.len()));
        let (fp, un) = rs.iter().fold((0, 0), |(f, u), r| (f + r.score.false_positives, u + r.score.unmatched));
        let pct = if def == 0 { "—".to_string() } else { format!("{:.0}%", hit as f64 / def as f64 * 100.0) };
        out.push_str(&format!("{:<22}{:>6}{:>9}{:>9}{:>12}{:>12}{:>9}\n", arm, rs.len(), pct, format!("{}/{}", hit, def), fp, un, unique.get(arm).copied().unwrap_or(0)));
    }
    out.push_str(&kind_recall(runs, &arms));
    out.push_str("  unique = planted defects caught by only one arm, over cases that two or more arms ran. A false positive is a flagged decoy (correct code that looks suspicious); unmatched is a finding on neither.\n");
    out.push_str(&format!("  {}\n", DISCLAIMER.replace(" Scripted reviewers are functions, not models.", "")));
    out
}

/// Recall by defect kind and arm, lowest first, so the kinds that separate seats (or that none finds) show. Empty when no run
/// carries per-kind totals (rows written before they existed).
fn kind_recall(runs: &[RunReview], arms: &BTreeSet<&str>) -> String {
    let mut by: BTreeMap<&str, BTreeMap<&str, (usize, usize)>> = BTreeMap::new();
    for r in runs {
        for (kind, total) in &r.score.kinds {
            let hit = r.score.caught.iter().filter(|id| id.split_once(':').is_some_and(|(_, k)| k == kind)).count();
            let cell = by.entry(kind.as_str()).or_default().entry(r.arm).or_default();
            *cell = (cell.0 + hit, cell.1 + total);
        }
    }
    if by.is_empty() {
        return String::new();
    }
    let rate = |m: &BTreeMap<&str, (usize, usize)>| -> f64 {
        let (h, t) = m.values().fold((0, 0), |(h, t), c| (h + c.0, t + c.1));
        if t == 0 { 1.0 } else { h as f64 / t as f64 }
    };
    let mut kinds: Vec<&str> = by.keys().copied().collect();
    kinds.sort_by(|a, b| rate(&by[a]).partial_cmp(&rate(&by[b])).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b)));
    let mut out = String::from("\n  recall by defect kind (hardest first; caught/planted per arm)\n");
    out.push_str(&format!("  {:<22}", "kind"));
    for a in arms {
        out.push_str(&format!("{:>14}", a.chars().take(13).collect::<String>()));
    }
    out.push('\n');
    for k in kinds {
        out.push_str(&format!("  {:<22}", k));
        for a in arms {
            out.push_str(&format!("{:>14}", by[k].get(a).map_or("-".to_string(), |(h, t)| format!("{}/{}", h, t))));
        }
        out.push('\n');
    }
    out
}

// ── a case for a real model ─────────────────────────────────────────────────────────────────────────────────────

const FILE: &str = "review.py";
const KEY_FILE: &str = "review-key.json";

fn format_rules(d: Difficulty) -> String {
    let says = if d == Difficulty::Easy { "its docstring says" } else { "its docstring or a comment says, or what a function it calls promises" };
    format!("Answer with one line per defect, in exactly this form and nothing else on that line:\nFINDING <line number> <kind>: <one sentence>\nwhere <kind> is one of: {}. The line number is the file's own (the first line is 1). Report only real defects, where the code does not do what {}; code that looks odd but is correct is not a defect. If you find none, say so.", d.kinds().join(", "), says)
}

fn prompt(d: Difficulty) -> String {
    format!("Review @project/{}/{}. It is a small module whose functions each have a docstring stating what they should do. {}", crate::train_run::REPO_DIR, FILE, format_rules(d))
}

/// What an inline case asks: the module is in the prompt, with line numbers, so a seat with no tools can take it.
fn inline_prompt(source: &str, d: Difficulty) -> String {
    let numbered = numbered(source);
    format!("Below is a small Python module with line numbers. Each function has a docstring stating what it should do. Review it. {}\n\n---\n{}---", format_rules(d), numbered)
}

/// The numbered module, as a file for the project when the inline prompt would be too long for the task envelope.
const NUMBERED_FILE: &str = "review.numbered.txt";

fn numbered(source: &str) -> String {
    source.lines().enumerate().map(|(i, l)| format!("{:>3} | {}\n", i + 1, l)).collect()
}

/// The same ask when the numbered module sits in a project file: Delphi hands a seat with no tools that file as a slice.
fn pointer_prompt(d: Difficulty) -> String {
    format!("Below, as a context slice, is a small Python module with line numbers, from @project/{}/{} (each line is `<number> | <code>`). Each function has a docstring stating what it should do. Review it. {}", crate::train_run::REPO_DIR, NUMBERED_FILE, format_rules(d))
}

/// What a generated case is made of.
#[derive(Debug, Clone, Copy)]
pub struct Params<'a> {
    pub seed: u64,
    pub k: usize,
    pub decoys: usize,
    pub split: &'a str,
    pub inline: bool,
    pub min_recall: f64,
    pub max_fp: u64,
}

/// Write a case directory (`case.toml`, `fixture/review.py`, `review-key.json`) for `delphi train run`. The key sits outside the
/// fixture, so the model under test never sees it.
#[cfg(test)]
pub fn write_case(dir: &Path, p: &Params) -> Result<(), String> {
    write_case_at(dir, p, Difficulty::Easy)
}

/// [`write_case`] at a difficulty. Easy writes exactly what `write_case` always wrote.
pub fn write_case_at(dir: &Path, p: &Params, difficulty: Difficulty) -> Result<(), String> {
    let Params { seed, k, decoys, split, inline, min_recall, max_fp } = *p;
    let art = generate_at(seed, k, decoys, difficulty);
    let (k, decoys) = (art.key.defects.len(), art.key.decoys.len());
    let id = dir.file_name().and_then(|s| s.to_str()).ok_or("the case directory has no name")?;
    fs::create_dir_all(dir.join("fixture")).map_err(|e| format!("{}: {}", dir.display(), e))?;
    fs::write(dir.join("fixture").join(FILE), &art.source).map_err(|e| e.to_string())?;
    let mut key_json = art.key.to_json(seed, FILE);
    if difficulty != Difficulty::Easy {
        key_json["difficulty"] = json!(difficulty.name());
    }
    fs::write(dir.join(KEY_FILE), serde_json::to_string_pretty(&key_json).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
    let prompt_toml = if inline {
        let mut p = inline_prompt(&art.source, difficulty);
        if p.len() > crate::train_run::INLINE_PROMPT_MAX_BYTES {
            // Too long for the 8,000-byte envelope: the module goes to the project, the prompt points at it. The key stays outside.
            fs::write(dir.join("fixture").join(NUMBERED_FILE), numbered(&art.source)).map_err(|e| e.to_string())?;
            p = pointer_prompt(difficulty);
        }
        if p.contains("'''") {
            return Err("the artifact contains three single quotes and cannot be written as a literal TOML string".into());
        }
        format!("'''\n{}'''", p)
    } else {
        format!("'''\n{}\n'''", prompt(difficulty))
    };
    let toml = format!(
        "id = \"{id}\"\nsuite = \"review\"\nkind = \"task\"\nrole = \"red-team\"\nsplit = \"{split}\"\ntags = [\"seeded-defects\"]\nmax_cost_usd = 1.0\ntimeout_secs = 900\nprompt = {prompt}\n\n# Seed {seed}: {k} planted defects, {decoys} decoys{level} (`delphi train defects gen` made this case). The thresholds are this case's pass bar;\n# the recall and false-positive figures are diagnostics in the report, never a seat score (specs/078).\n[[check]]\nkind = \"review_findings\"\npath = \"{key}\"\nmin_recall = {min_recall}\nmax_false_positives = {max_fp}\n",
        level = if difficulty == Difficulty::Easy { String::new() } else { format!(", {} difficulty", difficulty.name()) }, id = id, split = split, prompt = prompt_toml, seed = seed, k = k, decoys = decoys, key = KEY_FILE, min_recall = min_recall, max_fp = max_fp
    );
    fs::write(dir.join("case.toml"), toml).map_err(|e| e.to_string())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

const USAGE: &str = "delphi train defects run [--seed <n>] [--difficulty easy|medium|hard] [--defects <1-8|16|24>] [--decoys <0-8|16|24>] [--json]\n  delphi train defects gen [--seed <n>] [--difficulty easy|medium|hard] [--defects <1-8|16|24>] [--decoys <0-8|16|24>] [--split dev|holdout] [--min-recall <0-1>] [--max-false-positives <n>] [--inline] --out <new case dir>\n  run scores scripted reviewers (no model, no cost). gen writes a case for `delphi train run` (suite `review`); --inline puts the numbered module in the prompt, for a seat with no tools (above 6000 bytes it goes to a project file the prompt points at). The figures are diagnostics, not a seat score. --difficulty sets the counts (easy 5/4, the default; medium up to 16 and hard up to 24 of each), the kinds and the size of the module.";

/// `delphi train defects run|gen ...` (`args[0]` is `defects`).
pub fn cli(args: &[String], suites: Option<&Path>) -> i32 {
    let sub = args.get(1).map(String::as_str);
    let num = |name: &str, default: u64, lo: u64, hi: u64| -> Result<u64, String> {
        let n = flag(args, name).map_or(Ok(default), |v| v.parse::<u64>().map_err(|_| format!("{} must be a whole number", name)))?;
        if (lo..=hi).contains(&n) { Ok(n) } else { Err(format!("{} must be between {} and {}", name, lo, hi)) }
    };
    let difficulty = match flag(args, "--difficulty") {
        None => Difficulty::Easy,
        Some(v) => match Difficulty::parse(&v) {
            Some(d) => d,
            None => {
                eprintln!("delphi train defects: --difficulty must be easy, medium or hard");
                return 2;
            }
        },
    };
    let (dk, dd) = difficulty.defaults();
    let cap = difficulty.cap() as u64;
    let (seed, k, decoys) = match (num("--seed", 1, 0, u64::MAX), num("--defects", dk as u64, 1, cap), num("--decoys", dd as u64, 0, cap)) {
        (Ok(s), Ok(k), Ok(d)) => (s, k as usize, d as usize),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            eprintln!("delphi train defects: {}", e);
            return 2;
        }
    };
    match sub {
        Some("run") => {
            let art = generate_at(seed, k, decoys, difficulty);
            let rows: Vec<(Reviewer, Score)> = Reviewer::ALL.iter().map(|r| (*r, score(&art.key, &r.answer(&art.key)))).collect();
            if args.iter().any(|a| a == "--json") {
                let j = json!({"seed": seed, "difficulty": difficulty.name(), "defects": k, "decoys": decoys, "note": DISCLAIMER,
                               "reviewers": rows.iter().map(|(r, s)| json!({"reviewer": r.name(), "recall": s.recall(), "caught": s.caught.len(), "false_positives": s.false_positives, "unmatched": s.unmatched})).collect::<Vec<_>>()});
                println!("{}", serde_json::to_string_pretty(&j).unwrap_or_default());
            } else {
                print!("{}", table(seed, &art.key, &rows));
            }
            0
        }
        Some("gen") => {
            let split = flag(args, "--split").unwrap_or_else(|| "dev".into());
            if !["dev", "holdout"].contains(&split.as_str()) {
                eprintln!("delphi train defects: --split must be dev or holdout");
                return 2;
            }
            let min_recall = match flag(args, "--min-recall").map_or(Ok(0.5), |v| v.parse::<f64>()) {
                Ok(m) if m > 0.0 && m <= 1.0 => m,
                _ => {
                    eprintln!("delphi train defects: --min-recall must be above 0 and at most 1");
                    return 2;
                }
            };
            let max_fp = match num("--max-false-positives", 2, 0, 64) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("delphi train defects: {}", e);
                    return 2;
                }
            };
            let Some(out) = flag(args, "--out").map(PathBuf::from) else {
                eprintln!("delphi train defects gen needs --out <new case dir>, such as {}/review/cases/rev-{}-{}\n  {}", suites.map_or("dev/train/suites".into(), |s| s.display().to_string()), seed, k, USAGE);
                return 2;
            };
            if out.exists() {
                eprintln!("delphi train defects: {} already exists; give a new directory", out.display());
                return 2;
            }
            match write_case_at(&out, &Params { seed, k, decoys, split: &split, inline: args.iter().any(|a| a == "--inline"), min_recall, max_fp }, difficulty) {
                Ok(()) => {
                    println!("wrote {}: {} planted defects, {} decoys (seed {}, {}); `delphi train validate` checks it, `delphi train run` runs it on an arm", out.display(), k, decoys, seed, difficulty.name());
                    0
                }
                Err(e) => {
                    eprintln!("delphi train defects: {}", e);
                    1
                }
            }
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u64) -> (Artifact, Key) {
        let a = generate(seed, 5, 4);
        let k = a.key.clone();
        (a, k)
    }

    #[test]
    fn generation_is_deterministic_per_seed_and_the_key_points_at_the_planted_lines() {
        let (a, b, c) = (generate(7, 5, 4), generate(7, 5, 4), generate(8, 5, 4));
        assert_eq!((&a.source, &a.key), (&b.source, &b.key));
        assert_ne!(a.source, c.source, "a new seed is a new case");
        assert_eq!((a.key.defects.len(), a.key.decoys.len()), (5, 4));
        let lines: Vec<&str> = a.source.lines().collect();
        // Each recorded line holds the construct the key says it does.
        let marks = [("off-by-one", "range(1, len(rows))"), ("inverted-condition", "balance >= amount"), ("missing-error-check", "return row[0]"), ("wrong-operator", "value < low and value > high"), ("resource-leak", "open(path)"), ("swapped-arguments", "copyfile(dst, src)"), ("stale-constant", "start = page * "), ("injection", "+ name +")];
        for d in &a.key.defects {
            let mark = marks.iter().find(|m| m.0 == d.kind).unwrap().1;
            assert!(lines[d.line - 1].contains(mark), "{} at {}: {}", d.kind, d.line, lines[d.line - 1]);
        }
        for d in &a.key.decoys {
            assert!(!lines[d.line - 1].trim().is_empty(), "decoy {} at {}", d.imitates, d.line);
        }
        let distinct: BTreeSet<&str> = a.key.defects.iter().map(|d| d.kind.as_str()).collect();
        assert_eq!(distinct.len(), 5, "kinds are distinct");
        // Function names are unique, so no two constructs collide.
        let defs: Vec<&str> = lines.iter().filter(|l| l.starts_with("def ")).copied().collect();
        assert_eq!(defs.iter().collect::<BTreeSet<_>>().len(), defs.len());
        // Targets are far enough apart that a +-1 window never covers two of them.
        let mut at: Vec<usize> = a.key.defects.iter().map(|d| d.line).chain(a.key.decoys.iter().map(|d| d.line)).collect();
        at.sort();
        assert!(at.windows(2).all(|w| w[1] - w[0] > 2), "{:?}", at);
    }

    #[test]
    fn every_defect_and_decoy_template_is_valid_python_shaped_and_hits_its_own_line() {
        // The recorded line of each template is the line holding its target text.
        for t in DEFECTS.iter().chain(DECOYS.iter()) {
            assert!(t.target < t.lines.len() && !t.lines[t.target].is_empty(), "{}", t.kind);
        }
        for s in 0..40u64 {
            let a = generate(s, 8, 8);
            let n = a.source.lines().count();
            assert!(a.key.defects.iter().map(|d| d.line).chain(a.key.decoys.iter().map(|d| d.line)).all(|l| l >= 1 && l <= n));
            assert!(!a.source.contains("{f}") && !a.source.contains("{C}") && !a.source.contains("{n}") && !a.source.contains("{m}"));
            assert!(!a.source.contains("'''"), "fits a TOML literal string");
        }
    }

    #[test]
    fn a_perfect_answer_catches_everything_with_no_false_positives() {
        let (_, k) = key(3);
        let s = score(&k, &Reviewer::Thorough.answer(&k));
        assert_eq!((s.caught.len(), s.defects, s.false_positives, s.unmatched, s.recall()), (5, 5, 0, 0, 1.0));
        assert!(verdict(&s, 1.0, 0).is_ok());
    }

    #[test]
    fn a_partial_answer_scores_its_recall_and_a_line_one_off_still_counts_but_two_off_does_not() {
        let (_, k) = key(3);
        let (d0, d1, d2) = (&k.defects[0], &k.defects[1], &k.defects[2]);
        let ans = format!("FINDING {} {}: x\n- FINDING {} {}: y\n  FINDING {} {}: z\n", d0.line + 1, d0.kind, d1.line - 1, d1.kind, d2.line + 2, d2.kind);
        let s = score(&k, &ans);
        assert_eq!((s.caught.len(), s.false_positives), (2, 0));
        assert_eq!(s.unmatched, 1, "two lines off holds nothing");
        assert!((s.recall() - 0.4).abs() < 1e-9);
        assert!(verdict(&s, 0.5, 2).unwrap_err().contains("caught 2 of 5 planted defects (40%), wanted 50%"));
        // A neighbouring kind the key accepts counts; a kind it does not accept is unmatched, not a catch.
        let off = k.defects.iter().find(|d| d.kind == "off-by-one").map(|d| d.line);
        let swapped = k.defects.iter().find(|d| d.kind == "swapped-arguments").map(|d| d.line);
        if let Some(l) = off {
            assert_eq!(score(&k, &format!("FINDING {} wrong-operator: x", l)).caught.len(), 1);
        }
        if let Some(l) = swapped {
            let s = score(&k, &format!("FINDING {} resource-leak: x", l));
            assert_eq!((s.caught.len(), s.unmatched), (0, 1));
        }
        // A second finding for a defect already caught is ignored, not counted against the seat.
        let twice = format!("FINDING {} {}: a\nFINDING {} {}: b\n", d0.line, d0.kind, d0.line, d0.kind);
        let s = score(&k, &twice);
        assert_eq!((s.caught.len(), s.false_positives, s.unmatched, s.findings), (1, 0, 0, 2));
    }

    #[test]
    fn flagging_decoys_is_a_false_positive_and_does_not_help_recall() {
        let (_, k) = key(5);
        let s = score(&k, &Reviewer::Plausible.answer(&k));
        assert_eq!((s.caught.len(), s.false_positives), (0, 4));
        assert!(verdict(&s, 0.5, 4).unwrap_err().contains("caught 0 of 5"));
        // Flagging everything catches everything and pays for it in false positives.
        let all = score(&k, &Reviewer::Suspicious.answer(&k));
        assert_eq!((all.caught.len(), all.false_positives), (5, 4));
        assert!(verdict(&all, 1.0, 2).unwrap_err().contains("4 decoy(s) flagged, at most 2 allowed"));
        assert!(verdict(&all, 1.0, 4).is_ok());
    }

    #[test]
    fn garbage_is_not_a_finding() {
        let (_, k) = key(5);
        for bad in ["", "The code looks fine to me.", "FINDING at line 12: off by one", "finding: 12 off-by-one", "Line 12 off-by-one: x", "FINDING 12: no kind"] {
            let s = score(&k, bad);
            assert_eq!((s.findings, s.caught.len(), s.false_positives, s.unmatched), (0, 0, 0, 0), "{:?}", bad);
            assert!(verdict(&s, 0.1, 9).unwrap_err().contains("no `FINDING"));
        }
        // Case and list markers are tolerated; prose around the lines is ignored.
        let l = k.defects[0].line;
        let s = score(&k, &format!("Here is my review.\n* finding {} {}: ok\nDone.", l, k.defects[0].kind.to_uppercase()));
        assert_eq!(s.caught.len(), 1);
    }

    #[test]
    fn the_key_is_not_in_the_prompt_and_the_inline_prompt_holds_the_whole_module() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("suites/review/cases/rev-3-5i");
        write_case(&dir, &Params { seed: 3, k: 5, decoys: 4, split: "dev", inline: true, min_recall: 0.5, max_fp: 2 }).unwrap();
        let case = crate::train::load_case(&dir, "review").unwrap();
        let prompt = case.prompt.clone().unwrap();
        let art = generate(3, 5, 4);
        // Every line of the module is in the prompt, behind its number; nothing the key adds is.
        for (i, l) in art.source.lines().enumerate() {
            assert!(prompt.contains(&format!("{:>3} | {}\n", i + 1, l)), "line {}", i + 1);
        }
        assert!(prompt.contains("FINDING <line number> <kind>") && prompt.contains("injection") && !prompt.contains("@project/"));
        for d in &art.key.decoys {
            assert!(!d.why.is_empty() && !prompt.contains(&d.why));
        }
        assert!(!prompt.contains("planted") && !prompt.contains("decoy") && !prompt.contains(KEY_FILE));
        assert!(fs::read_to_string(dir.join(KEY_FILE)).unwrap().contains("\"decoys\""));
        for f in fs::read_dir(dir.join("fixture")).unwrap().flatten() {
            assert!(f.file_name() == FILE, "only the module is in the fixture");
        }
        assert_eq!(fs::read_to_string(dir.join("fixture").join(FILE)).unwrap(), art.source);
        // The ordinary case names the file and carries no code.
        let plain = t.path().join("suites/review/cases/rev-3-5");
        write_case(&plain, &Params { seed: 3, k: 5, decoys: 4, split: "dev", inline: false, min_recall: 0.5, max_fp: 2 }).unwrap();
        let p = crate::train::load_case(&plain, "review").unwrap().prompt.unwrap();
        assert!(p.contains("@project/repo/review.py") && !p.contains("def "), "{}", p);
    }

    #[test]
    fn the_check_parses_from_case_toml_and_refuses_bad_thresholds() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("suites/review/cases/rev-2-4");
        write_case(&dir, &Params { seed: 2, k: 4, decoys: 3, split: "holdout", inline: false, min_recall: 0.75, max_fp: 1 }).unwrap();
        let case = crate::train::load_case(&dir, "review").unwrap();
        assert!(matches!(&case.checks[0], crate::train::Check::ReviewFindings { file, min_recall, max_false_positives } if file == KEY_FILE && *min_recall == 0.75 && *max_false_positives == 1), "{:?}", case.checks);
        let good = fs::read_to_string(dir.join("case.toml")).unwrap();
        for (bad, want) in [
            (good.replace("path = \"review-key.json\"\n", ""), "needs `path`"),
            (good.replace("path = \"review-key.json\"", "path = \"../k.json\""), "must be relative"),
            (good.replace("min_recall = 0.75", "min_recall = 0.0"), "needs `min_recall` above 0"),
            (good.replace("min_recall = 0.75", "min_recall = 1.5"), "needs `min_recall` above 0"),
            (good.replace("max_false_positives = 1\n", ""), "needs `max_false_positives`"),
            (good.replace("max_false_positives = 1", "max_false_positives = -1"), "needs `max_false_positives`"),
        ] {
            fs::write(dir.join("case.toml"), &bad).unwrap();
            let e = crate::train::load_case(&dir, "review").unwrap_err().join("; ");
            assert!(e.contains(want), "{} in {}", want, e);
        }
    }

    #[test]
    fn the_report_section_prints_recall_false_positives_and_unique_catches() {
        let s = |caught: &[&str], fp: usize, un: usize| Score { findings: caught.len() + fp + un, defects: 4, caught: caught.iter().map(|c| c.to_string()).collect(), false_positives: fp, unmatched: un, kinds: Default::default() };
        let (a1, a2, b1, b2) = (s(&["10:x", "20:y", "30:z"], 0, 1), s(&["10:x", "40:w", "50:q"], 1, 0), s(&["10:x", "20:y"], 3, 2), s(&["20:y", "10:x"], 2, 0));
        let runs = [
            RunReview { case: "review/c1", arm: "tool", score: &a1 },
            RunReview { case: "review/c2", arm: "tool", score: &a2 },
            RunReview { case: "review/c1", arm: "toolless", score: &b1 },
            RunReview { case: "review/c2", arm: "toolless", score: &b2 },
        ];
        let out = report_section(&runs);
        let cells = |prefix: &str| -> Vec<String> { out.lines().find(|l| l.starts_with(prefix)).unwrap().split_whitespace().map(String::from).collect() };
        assert_eq!(cells("arm"), ["arm", "runs", "recall", "caught", "false", "pos.", "unmatched", "unique"]);
        // c1: 30:z only the tool arm. c2: 40:w and 50:q only the tool arm, 20:y only the toolless arm.
        assert_eq!(cells("tool "), ["tool", "2", "75%", "6/8", "1", "1", "3"], "{}", out);
        assert_eq!(cells("toolless"), ["toolless", "2", "50%", "4/8", "5", "2", "1"], "{}", out);
        assert!(out.starts_with("\nSEEDED-DEFECT REVIEW (planted defects found, by arm; diagnostics only)\n"));
        assert!(out.contains("not a seat score") && out.contains("Council decision") && out.contains("specs/078"), "{}", out);
        assert!(report_section(&[]).is_empty());
        // One arm alone has no unique catches: there is nobody to be unique against.
        let solo = report_section(&runs[..2]);
        assert!(solo.lines().any(|l| l.starts_with("tool ") && l.split_whitespace().last() == Some("0")), "{}", solo);
    }

    #[test]
    fn the_scripted_reviewers_tell_the_cases_apart_and_the_command_checks_its_arguments() {
        let art = generate(1, 5, 4);
        let by: BTreeMap<&str, Score> = Reviewer::ALL.iter().map(|r| (r.name(), score(&art.key, &r.answer(&art.key)))).collect();
        assert_eq!(by["thorough"].recall(), 1.0);
        assert_eq!((by["suspicious"].recall(), by["suspicious"].false_positives), (1.0, 4));
        assert_eq!((by["plausible"].caught.len(), by["plausible"].false_positives), (0, 4));
        assert_eq!(by["mislabelling"].caught.len(), 0);
        assert_eq!(by["silent"].findings, 0);
        let rows: Vec<(Reviewer, Score)> = Reviewer::ALL.iter().map(|r| (*r, by[r.name()].clone())).collect();
        let t = table(1, &art.key, &rows);
        for want in ["SEEDED-DEFECT REVIEW (seed 1, 5 planted defects, 4 decoys)", "thorough", "5/5", "suspicious", "silent", "not a seat score"] {
            assert!(t.contains(want), "{} in\n{}", want, t);
        }
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("new-case");
        assert_eq!(cli(&a(&["defects", "gen", "--seed", "3", "--inline", "--out", out.to_str().unwrap()]), None), 0);
        assert!(fs::read_to_string(out.join("case.toml")).unwrap().contains("Below is a small Python module"));
        assert_eq!(cli(&a(&["defects", "gen", "--out", out.to_str().unwrap()]), None), 2, "an existing directory is refused");
        assert_eq!(cli(&a(&["defects", "gen"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--defects", "0"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--decoys", "9"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--seed", "x"]), None), 2);
        assert_eq!(cli(&a(&["defects", "gen", "--min-recall", "0", "--out", tmp.path().join("z").to_str().unwrap()]), None), 2);
        assert_eq!(cli(&a(&["defects", "run"]), None), 0);
        assert_eq!(cli(&a(&["defects", "run", "--json"]), None), 0);
        assert_eq!(cli(&a(&["defects"]), None), 2);
    }

    // ── difficulty ──────────────────────────────────────────────────────────────────────────────────────────────

    #[test]
    fn easy_is_the_original_case_and_the_other_levels_are_deterministic_and_larger() {
        for s in [1u64, 7, 42] {
            let (e, e2) = (generate(s, 5, 4), generate_at(s, 5, 4, Difficulty::Easy));
            assert_eq!((&e.source, &e.key), (&e2.source, &e2.key), "easy is what `generate` always was");
            assert!(e.key.defects.iter().all(|d| d.also.is_none()) && !e.source.contains("import time"), "easy has none of the new constructs");
        }
        for d in [Difficulty::Medium, Difficulty::Hard] {
            let (dk, dd) = d.defaults();
            let (a, b, c) = (generate_at(7, dk, dd, d), generate_at(7, dk, dd, d), generate_at(8, dk, dd, d));
            assert_eq!((&a.source, &a.key), (&b.source, &b.key), "{:?}", d);
            assert_ne!(a.source, c.source);
            assert_eq!((a.key.defects.len(), a.key.decoys.len()), (dk, dd));
        }
        // The hard default is long enough that an inline prompt is a real read; easy is a fraction of it.
        for s in 0..12u64 {
            let (h, m, e) = (generate_at(s, 20, 20, Difficulty::Hard), generate_at(s, 10, 8, Difficulty::Medium), generate(s, 5, 4));
            let (hl, ml, el) = (h.source.lines().count(), m.source.lines().count(), e.source.lines().count());
            assert!(hl > 300 && hl > ml && ml > el, "seed {}: {} {} {}", s, hl, ml, el);
        }
        // Counts clamp to the level's cap.
        let max = generate_at(3, 99, 99, Difficulty::Hard);
        assert_eq!((max.key.defects.len(), max.key.decoys.len()), (24, 24));
        assert!(generate_at(3, 99, 99, Difficulty::Hard).source.lines().count() > 350);
        assert_eq!(generate_at(3, 99, 99, Difficulty::Medium).key.defects.len(), 16);
        assert_eq!(generate_at(3, 99, 99, Difficulty::Easy).key.defects.len(), 8);
    }

    #[test]
    fn every_hard_kind_generates_scores_and_its_decoy_twin_is_not_flagged_by_a_perfect_answer() {
        // 24 of 24 uses every template once, as defect and as decoy.
        for seed in [1u64, 2, 3] {
            let a = generate_at(seed, 24, 24, Difficulty::Hard);
            let lines: Vec<&str> = a.source.lines().collect();
            let kinds: BTreeSet<&str> = a.key.defects.iter().map(|d| d.kind.as_str()).collect();
            let want: BTreeSet<&str> = KINDS.iter().chain(HARD_KINDS.iter()).copied().collect();
            assert_eq!(kinds, want, "every kind is planted");
            assert!(!a.source.contains("{f}") && !a.source.contains("{C}") && !a.source.contains("'''"));
            for d in &a.key.defects {
                assert!(!lines[d.line - 1].trim().is_empty() && d.also.is_none_or(|l| !lines[l - 1].trim().is_empty()), "{:?}", d);
            }
            for d in &a.key.decoys {
                assert!(!d.why.is_empty() && !lines[d.line - 1].trim().is_empty(), "{:?}", d);
            }
            let s = score(&a.key, &Reviewer::Thorough.answer(&a.key));
            assert_eq!((s.caught.len(), s.defects, s.false_positives, s.unmatched), (24, 24, 0, 0), "seed {}", seed);
            assert_eq!(s.kinds.len(), 15);
            // No two constructs share a +-1 window: every defect and decoy line (and second line) is 3 or more from any other's.
            let mut at: Vec<(usize, usize)> = Vec::new();
            for (i, d) in a.key.defects.iter().enumerate() {
                at.push((i, d.line));
                at.extend(d.also.map(|l| (i, l)));
            }
            for (i, d) in a.key.decoys.iter().enumerate() {
                at.push((100 + i, d.line));
                at.extend(d.also.map(|l| (100 + i, l)));
            }
            for x in &at {
                for y in &at {
                    assert!(x.0 == y.0 || x.1.abs_diff(y.1) > 2, "{:?} {:?}", x, y);
                }
            }
            // Flagging every decoy is a false positive each and catches nothing, on either of its lines.
            let all_decoys: String = a.key.decoys.iter().flat_map(|d| [d.line].into_iter().chain(d.also)).map(|l| format!("FINDING {} injection: x\n", l)).collect();
            let s = score(&a.key, &all_decoys);
            assert_eq!((s.caught.len(), s.false_positives), (0, a.key.decoys.iter().map(|d| 1 + d.also.is_some() as usize).sum::<usize>()), "{}", all_decoys);
        }
    }

    #[test]
    fn a_cross_function_defect_is_caught_on_either_recorded_line_and_the_two_are_far_apart() {
        for seed in 0..10u64 {
            let a = generate_at(seed, 24, 24, Difficulty::Hard);
            let cross: Vec<&Defect> = a.key.defects.iter().filter(|d| d.kind == "contract-violation").collect();
            assert_eq!(cross.len(), 2);
            for d in cross {
                let also = d.also.expect("a cross-function defect records both lines");
                assert!(d.line.abs_diff(also) > 40, "seed {}: {} vs {}", seed, d.line, also);
                for line in [d.line, also, d.line + 1, also - 1] {
                    let s = score(&a.key, &format!("FINDING {} contract-violation: x", line));
                    assert_eq!((s.caught, s.unmatched), (vec![d.id()], 0), "line {} of {:?}", line, d);
                }
                let s = score(&a.key, &format!("FINDING {} contract-violation: x", also + 2));
                assert!(s.caught.is_empty(), "two lines off is not a catch");
            }
        }
        // A check-then-act defect is caught on the check or the act.
        let a = generate_at(1, 24, 0, Difficulty::Hard);
        let race = a.key.defects.iter().find(|d| d.kind == "race" && d.also.is_some()).unwrap();
        for l in [race.line, race.also.unwrap()] {
            assert_eq!(score(&a.key, &format!("FINDING {} race: x", l)).caught.len(), 1);
        }
        // Neighbour kinds are accepted only where two honest names exist.
        let t = a.key.defects.iter().find(|d| d.kind == "swallowed-error").unwrap();
        assert_eq!(score(&a.key, &format!("FINDING {} missing-error-check: x", t.line)).caught.len(), 1);
        assert_eq!(score(&a.key, &format!("FINDING {} race: x", t.line)).unmatched, 1);
    }

    #[test]
    fn partial_mislabelled_and_garbage_answers_score_as_expected_on_a_hard_case() {
        let a = generate_at(9, 20, 20, Difficulty::Hard);
        let k = &a.key;
        let half: String = k.defects.iter().step_by(2).map(|d| format!("FINDING {} {}: x\n", d.line, d.kind)).collect();
        let s = score(k, &half);
        assert_eq!((s.caught.len(), s.false_positives, s.unmatched), (10, 0, 0));
        assert!((s.recall() - 0.5).abs() < 1e-9);
        assert!(verdict(&s, 0.6, 2).is_err() && verdict(&s, 0.5, 2).is_ok());
        assert_eq!(score(k, &Reviewer::Mislabelling.answer(k)).caught.len(), 0);
        assert_eq!(score(k, &Reviewer::Silent.answer(k)).findings, 0);
        let plausible = score(k, &Reviewer::Plausible.answer(k));
        assert_eq!((plausible.caught.len(), plausible.false_positives), (0, 20));
        let noisy = score(k, &Reviewer::Suspicious.answer(k));
        assert_eq!((noisy.caught.len(), noisy.false_positives), (20, 20));
        for bad in ["", "FINDING at line 12: x", "FINDING 12: no kind"] {
            assert_eq!(score(k, bad).findings, 0, "{:?}", bad);
        }
    }

    #[test]
    fn a_hard_case_keeps_its_key_out_of_the_prompt_and_lists_the_new_kinds() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("suites/review/cases/rev-3-18h");
        let p = Params { seed: 3, k: 20, decoys: 20, split: "dev", inline: true, min_recall: 0.5, max_fp: 2 };
        write_case_at(&dir, &p, Difficulty::Hard).unwrap();
        let case = crate::train::load_case(&dir, "review").unwrap();
        let prompt = case.prompt.clone().unwrap();
        let art = generate_at(3, 20, 20, Difficulty::Hard);
        // Hard is too long for the task envelope, so the numbered module is in the project file the prompt points at.
        let file = fs::read_to_string(dir.join("fixture").join(NUMBERED_FILE)).unwrap();
        for (i, l) in art.source.lines().enumerate() {
            assert!(file.contains(&format!("{:>3} | {}\n", i + 1, l)), "line {}", i + 1);
        }
        for kind in HARD_KINDS {
            assert!(prompt.contains(kind), "{}", kind);
        }
        for d in &art.key.decoys {
            assert!(!d.why.is_empty() && !prompt.contains(&d.why), "{}", d.why);
        }
        assert!(!prompt.contains("planted") && !prompt.contains("decoy") && !prompt.contains(KEY_FILE) && !prompt.contains("also_line"));
        let key_text = fs::read_to_string(dir.join(KEY_FILE)).unwrap();
        assert!(key_text.contains("\"difficulty\": \"hard\"") && key_text.contains("also_line"));
        let back = Key::from_json(&key_text).unwrap();
        assert_eq!(back.defects, art.key.defects);
        assert_eq!(back.decoys, art.key.decoys);
        // The same module and key again from the same arguments.
        let again = t.path().join("suites/review/cases/rev-3-18h2");
        write_case_at(&again, &p, Difficulty::Hard).unwrap();
        assert_eq!(fs::read_to_string(again.join("case.toml")).unwrap().replace("rev-3-18h2", "rev-3-18h"), fs::read_to_string(dir.join("case.toml")).unwrap());
        // The easy prompt still lists only the original eight kinds.
        let easy = t.path().join("suites/review/cases/rev-3-5i");
        write_case_at(&easy, &Params { k: 5, decoys: 4, ..p }, Difficulty::Easy).unwrap();
        let ep = crate::train::load_case(&easy, "review").unwrap().prompt.unwrap();
        assert!(!ep.contains("race") && !ep.contains("contract-violation"));
    }

    #[test]
    fn a_hard_inline_case_puts_the_module_in_a_project_file_the_prompt_points_at() {
        let t = tempfile::tempdir().unwrap();
        for (seed, k, decoys, d, name) in [(3u64, 20usize, 20usize, Difficulty::Hard, "h"), (4, 24, 24, Difficulty::Hard, "hmax")] {
            let dir = t.path().join(format!("suites/review/cases/{name}"));
            write_case_at(&dir, &Params { seed, k, decoys, split: "dev", inline: true, min_recall: 0.5, max_fp: 2 }, d).unwrap();
            let case = crate::train::load_case(&dir, "review").unwrap();
            let prompt = case.prompt.clone().unwrap();
            assert!(prompt.len() < 6000, "{}", prompt.len());
            // The pointer is one the engine reads, and the file it names holds the whole numbered module.
            let ptrs = delphi_engine::handoff::pointers_from_prompt(&prompt);
            assert_eq!(ptrs.len(), 1, "{:?}", ptrs);
            assert_eq!(ptrs[0].uri, format!("@project/{}/{}", crate::train_run::REPO_DIR, NUMBERED_FILE));
            let file = fs::read_to_string(dir.join("fixture").join(NUMBERED_FILE)).unwrap();
            let art = generate_at(seed, k, decoys, d);
            assert_eq!(file, numbered(&art.source));
            assert!(file.lines().count() <= 400, "the pointer slice reads at most 400 lines");
            // A tool-less seat scripted to answer from the slice it was handed can be scored.
            let first = art.key.defects[0].line;
            let reply = format!("FINDING {} {}: from the slice", first, art.key.defects[0].kind);
            assert!(file.lines().nth(first - 1).unwrap().starts_with(&format!("{:>3} | ", first)));
            assert!(score(&art.key, &reply).findings >= 1);
            // The key is neither in the pointer file nor in the prompt.
            let key_text = fs::read_to_string(dir.join(KEY_FILE)).unwrap();
            assert!(key_text.contains("also_line") && !file.contains("also_line") && !prompt.contains(KEY_FILE) && !prompt.contains("planted"));
            assert!(!dir.join("fixture").join(KEY_FILE).exists());
            assert!(crate::train::load_case(&dir, "review").is_ok());
        }
        // Easy and medium inline stay in the prompt, with no extra file.
        for (d, k, dd, name) in [(Difficulty::Easy, 5, 4, "e"), (Difficulty::Medium, 10, 8, "m")] {
            let dir = t.path().join(format!("suites/review/cases/{name}"));
            write_case_at(&dir, &Params { seed: 3, k, decoys: dd, split: "dev", inline: true, min_recall: 0.5, max_fp: 2 }, d).unwrap();
            let prompt = crate::train::load_case(&dir, "review").unwrap().prompt.unwrap();
            let art = generate_at(3, k, dd, d);
            assert_eq!(prompt, inline_prompt(&art.source, d));
            assert!(!dir.join("fixture").join(NUMBERED_FILE).exists() && !prompt.contains("@project/"));
        }
    }

    #[test]
    fn the_report_shows_recall_by_kind_hardest_first() {
        let kinds: BTreeMap<String, usize> = [("race".to_string(), 2), ("injection".to_string(), 2)].into_iter().collect();
        let s = |caught: &[&str]| Score { findings: caught.len(), defects: 4, caught: caught.iter().map(|c| c.to_string()).collect(), kinds: kinds.clone(), ..Score::default() };
        let (a, b) = (s(&["10:race", "20:injection", "30:injection"]), s(&["20:injection", "30:injection"]));
        let runs = [RunReview { case: "review/c1", arm: "armA", score: &a }, RunReview { case: "review/c1", arm: "armB", score: &b }];
        let out = report_section(&runs);
        let rows: Vec<Vec<&str>> = out.lines().skip_while(|l| !l.contains("recall by defect kind")).skip(2).map(|l| l.split_whitespace().collect()).collect();
        assert_eq!(rows[0], ["race", "1/2", "0/2"], "{}", out);
        assert_eq!(rows[1], ["injection", "2/2", "2/2"], "{}", out);
        // Old rows carry no per-kind totals and print no such table.
        let old = Score { findings: 1, defects: 4, caught: vec!["10:x".into()], ..Score::default() };
        assert!(!report_section(&[RunReview { case: "c", arm: "a", score: &old }]).contains("by defect kind"));
        // The figures survive a row round trip.
        assert_eq!(Score::from_note(&a.note()).unwrap(), a);
    }

    #[test]
    fn the_command_takes_a_difficulty_and_checks_its_caps() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(cli(&a(&["defects", "run", "--difficulty", "hard"]), None), 0);
        assert_eq!(cli(&a(&["defects", "run", "--difficulty", "medium", "--defects", "16", "--decoys", "16"]), None), 0);
        assert_eq!(cli(&a(&["defects", "run", "--difficulty", "hard", "--defects", "24", "--decoys", "24", "--json"]), None), 0);
        assert_eq!(cli(&a(&["defects", "run", "--difficulty", "hard", "--defects", "25"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--difficulty", "medium", "--defects", "17"]), None), 2);
        assert_eq!(cli(&a(&["defects", "run", "--defects", "9"]), None), 2, "easy keeps its cap of 8");
        assert_eq!(cli(&a(&["defects", "run", "--difficulty", "brutal"]), None), 2);
        let out = tmp.path().join("hard-case");
        assert_eq!(cli(&a(&["defects", "gen", "--seed", "4", "--difficulty", "hard", "--inline", "--out", out.to_str().unwrap()]), None), 0);
        let toml = fs::read_to_string(out.join("case.toml")).unwrap();
        assert!(toml.contains("20 planted defects, 20 decoys, hard difficulty") && toml.contains("contract-violation"), "{}", toml);
        assert!(crate::train::load_case(&out, "review").is_ok());
    }
}
