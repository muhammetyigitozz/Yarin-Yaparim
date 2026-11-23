use chrono::{NaiveDate, NaiveTime};
use rusqlite::{params, Connection, OptionalExtension};

pub struct Db {
    pub c: Connection,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub color: i32,
    pub open: i32,
}

#[derive(Clone, Debug)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub parent_id: Option<i64>,
    pub title: String,
    pub due_date: Option<NaiveDate>,
    pub due_time: Option<NaiveTime>,
    pub high: bool,
    pub done: bool,
}

fn row_task(r: &rusqlite::Row) -> rusqlite::Result<Task> {
    let dd: Option<String> = r.get(4)?;
    let dt: Option<String> = r.get(5)?;
    Ok(Task {
        id: r.get(0)?,
        project_id: r.get(1)?,
        parent_id: r.get(2)?,
        title: r.get(3)?,
        due_date: dd.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        due_time: dt.and_then(|s| NaiveTime::parse_from_str(&s, "%H:%M").ok()),
        high: r.get::<_, i64>(6)? != 0,
        done: r.get::<_, i64>(7)? != 0,
    })
}

const TASK_COLS: &str = "id, project_id, parent_id, title, due_date, due_time, priority, done";

impl Db {
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Db> {
        let c = Connection::open(path)?;
        c.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=3000;
             CREATE TABLE IF NOT EXISTS projects(
                id INTEGER PRIMARY KEY, name TEXT NOT NULL, color INTEGER NOT NULL DEFAULT 0,
                sort INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE IF NOT EXISTS tasks(
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                parent_id INTEGER REFERENCES tasks(id) ON DELETE CASCADE,
                title TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '',
                due_date TEXT, due_time TEXT,
                priority INTEGER NOT NULL DEFAULT 0,
                done INTEGER NOT NULL DEFAULT 0, done_at TEXT,
                sort INTEGER NOT NULL DEFAULT 0,
                notified INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE IF NOT EXISTS settings(k TEXT PRIMARY KEY, v TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS ix_tasks_proj ON tasks(project_id, parent_id, done, sort);",
        )?;
        Ok(Db { c })
    }

    /// Değişiklik algılama için (MCP gibi dış yazarlar).
    pub fn data_version(&self) -> i64 {
        self.c.query_row("PRAGMA data_version", [], |r| r.get(0)).unwrap_or(0)
    }

    pub fn setting(&self, k: &str) -> Option<String> {
        self.c.query_row("SELECT v FROM settings WHERE k=?", [k], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_setting(&self, k: &str, v: &str) {
        let _ = self.c.execute("INSERT INTO settings(k,v) VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [k, v]);
    }

    pub fn projects(&self) -> Vec<Project> {
        let mut s = self.c.prepare(
            "SELECT p.id, p.name, p.color,
               (SELECT COUNT(*) FROM tasks t WHERE t.project_id=p.id AND t.parent_id IS NULL AND t.done=0)
             FROM projects p ORDER BY p.sort, p.id").unwrap();
        s.query_map([], |r| Ok(Project { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, open: r.get(3)? }))
            .unwrap().filter_map(Result::ok).collect()
    }

    pub fn project_by_name(&self, name: &str) -> Option<i64> {
        let want = crate::parse::fold(name);
        self.projects().into_iter().find(|p| crate::parse::fold(&p.name) == want).map(|p| p.id)
    }

    pub fn add_project(&self, name: &str, color: i32) -> i64 {
        let sort: i64 = self.c.query_row("SELECT COALESCE(MAX(sort),0)+1 FROM projects", [], |r| r.get(0)).unwrap_or(0);
        self.c.execute("INSERT INTO projects(name,color,sort) VALUES(?,?,?)", params![name, color, sort]).unwrap();
        self.c.last_insert_rowid()
    }

    pub fn update_project(&self, id: i64, name: &str, color: i32) {
        let _ = self.c.execute("UPDATE projects SET name=?, color=? WHERE id=?", params![name, color, id]);
    }

    pub fn delete_project(&self, id: i64) {
        let _ = self.c.execute("DELETE FROM projects WHERE id=?", [id]);
    }

    pub fn tasks_of(&self, project: i64, parent: Option<i64>) -> Vec<Task> {
        let sql = format!(
            "SELECT {TASK_COLS} FROM tasks WHERE project_id=?1 AND parent_id IS ?2 ORDER BY done, CASE WHEN done=1 THEN done_at END DESC, sort, id");
        let mut s = self.c.prepare(&sql).unwrap();
        s.query_map(params![project, parent], row_task).unwrap().filter_map(Result::ok).collect()
    }

    pub fn subtasks(&self, parent: i64) -> Vec<Task> {
        let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE parent_id=? ORDER BY sort, id");
        let mut s = self.c.prepare(&sql).unwrap();
        s.query_map([parent], row_task).unwrap().filter_map(Result::ok).collect()
    }

    /// Tarihli, açık tüm üst görevler (Yakında görünümü).
    pub fn dated_open(&self) -> Vec<Task> {
        let sql = format!(
            "SELECT {TASK_COLS} FROM tasks WHERE parent_id IS NULL AND done=0 AND due_date IS NOT NULL
             ORDER BY due_date, COALESCE(due_time,'99:99'), priority DESC, sort");
        let mut s = self.c.prepare(&sql).unwrap();
        s.query_map([], row_task).unwrap().filter_map(Result::ok).collect()
    }

    pub fn task(&self, id: i64) -> Option<Task> {
        let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE id=?");
        self.c.query_row(&sql, [id], row_task).optional().ok().flatten()
    }

    pub fn add_task(&self, project: i64, parent: Option<i64>, title: &str,
                    date: Option<NaiveDate>, time: Option<NaiveTime>, high: bool) -> i64 {
        // Yeni görev listenin başına (sort en küçük - 1)
        let sort: i64 = self.c.query_row(
            "SELECT COALESCE(MIN(sort),1)-1 FROM tasks WHERE project_id=? AND parent_id IS ?",
            params![project, parent], |r| r.get(0)).unwrap_or(0);
        let sort = if parent.is_some() {
            self.c.query_row("SELECT COALESCE(MAX(sort),0)+1 FROM tasks WHERE parent_id=?", [parent], |r| r.get(0)).unwrap_or(0)
        } else { sort };
        self.c.execute(
            "INSERT INTO tasks(project_id,parent_id,title,due_date,due_time,priority,sort) VALUES(?,?,?,?,?,?,?)",
            params![project, parent, title,
                    date.map(|d| d.format("%Y-%m-%d").to_string()),
                    time.map(|t| t.format("%H:%M").to_string()),
                    high as i64, sort]).unwrap();
        self.c.last_insert_rowid()
    }

    pub fn set_done(&self, id: i64, done: bool) {
        let _ = self.c.execute(
            "UPDATE tasks SET done=?, done_at=CASE WHEN ?=1 THEN datetime('now') ELSE NULL END WHERE id=?",
            params![done as i64, done as i64, id]);
    }

    pub fn rename(&self, id: i64, title: &str) {
        let _ = self.c.execute("UPDATE tasks SET title=? WHERE id=?", params![title, id]);
    }

    pub fn set_due(&self, id: i64, date: Option<NaiveDate>, time: Option<NaiveTime>) {
        let _ = self.c.execute(
            "UPDATE tasks SET due_date=?, due_time=?, notified=0 WHERE id=?",
            params![date.map(|d| d.format("%Y-%m-%d").to_string()),
                    time.map(|t| t.format("%H:%M").to_string()), id]);
    }

    pub fn set_high(&self, id: i64, high: bool) {
        let _ = self.c.execute("UPDATE tasks SET priority=? WHERE id=?", params![high as i64, id]);
    }

    pub fn delete_task(&self, id: i64) {
        let _ = self.c.execute("DELETE FROM tasks WHERE id=?", [id]);
    }

    pub fn move_task(&self, id: i64, steps: i64) {
        let Some(t) = self.task(id) else { return };
        if t.done || steps == 0 { return; }
        let mut s = self.c.prepare(
            "SELECT id FROM tasks WHERE project_id=? AND parent_id IS ? AND done=0 ORDER BY sort, id").unwrap();
        let mut ids: Vec<i64> = s.query_map(params![t.project_id, t.parent_id], |r| r.get(0)).unwrap().filter_map(Result::ok).collect();
        drop(s);
        reorder(&mut ids, id, steps);
        let tx = self.c.unchecked_transaction().unwrap();
        for (i, tid) in ids.iter().enumerate() {
            tx.execute("UPDATE tasks SET sort=? WHERE id=?", params![i as i64, tid]).unwrap();
        }
        tx.commit().unwrap();
    }

    pub fn move_project(&self, id: i64, steps: i64) {
        let mut ids: Vec<i64> = self.projects().iter().map(|p| p.id).collect();
        reorder(&mut ids, id, steps);
        let tx = self.c.unchecked_transaction().unwrap();
        for (i, pid) in ids.iter().enumerate() {
            tx.execute("UPDATE projects SET sort=? WHERE id=?", params![i as i64, pid]).unwrap();
        }
        tx.commit().unwrap();
    }
}

fn reorder(ids: &mut Vec<i64>, id: i64, steps: i64) {
    if let Some(pos) = ids.iter().position(|x| *x == id) {
        let item = ids.remove(pos);
        let np = (pos as i64 + steps).clamp(0, ids.len() as i64) as usize;
        ids.insert(np, item);
    }
}
