// Stdio MCP sunucusu: `YarinYaparim --mcp`. Uygulamayla aynı veritabanını kullanır.
use crate::{db::Db, parse};
use chrono::{Local, NaiveDate};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn tools() -> Value {
    let s = |p: Value, r: Vec<&str>| json!({"type": "object", "properties": p, "required": r});
    json!([
        {"name": "list_projects", "description": "Projeleri ve açık görev sayılarını listeler.", "inputSchema": s(json!({}), vec![])},
        {"name": "add_project", "description": "Yeni proje ekler.", "inputSchema": s(json!({"name": {"type": "string"}, "color": {"type": "integer", "description": "0-6 renk sırası"}}), vec!["name"])},
        {"name": "list_tasks", "description": "Görevleri listeler. project verilmezse tüm projeler. due_within_days ile yalnız o kadar gün içinde (ve geciken) tarihli görevler.",
         "inputSchema": s(json!({"project": {"type": "string"}, "include_done": {"type": "boolean"}, "due_within_days": {"type": "integer"}}), vec![])},
        {"name": "add_task", "description": "Görev ekler. due Türkçe ifade olabilir: 'cuma 14:00', 'yarın', '2 Ara', '25.11.2026'. parent_id verilirse alt görev olur.",
         "inputSchema": s(json!({"title": {"type": "string"}, "project": {"type": "string"}, "due": {"type": "string"}, "high_priority": {"type": "boolean"}, "parent_id": {"type": "integer"}}), vec!["title"])},
        {"name": "update_task", "description": "Görevin başlığını, deadline'ını (boş metin = kaldır) veya önceliğini değiştirir.",
         "inputSchema": s(json!({"id": {"type": "integer"}, "title": {"type": "string"}, "due": {"type": "string"}, "high_priority": {"type": "boolean"}}), vec!["id"])},
        {"name": "complete_task", "description": "Görevi tamamlandı (veya done=false ile açık) yapar.", "inputSchema": s(json!({"id": {"type": "integer"}, "done": {"type": "boolean"}}), vec!["id"])},
        {"name": "delete_task", "description": "Görevi (alt görevleriyle) siler.", "inputSchema": s(json!({"id": {"type": "integer"}}), vec!["id"])}
    ])
}

fn task_json(db: &Db, t: &crate::db::Task, names: &[(i64, String)]) -> Value {
    let subs: Vec<Value> = db.subtasks(t.id).iter().map(|s| json!({"id": s.id, "title": s.title, "done": s.done})).collect();
    json!({"id": t.id, "title": t.title, "project": names.iter().find(|p| p.0 == t.project_id).map(|p| p.1.clone()),
        "due_date": t.due_date.map(|d| d.to_string()), "due_time": t.due_time.map(|x| x.format("%H:%M").to_string()),
        "high_priority": t.high, "done": t.done, "subtasks": subs})
}

fn call(db: &Db, name: &str, a: &Value) -> Result<Value, String> {
    let n = Local::now().naive_local();
    let (today, now) = (n.date(), n.time());
    let names: Vec<(i64, String)> = db.projects().into_iter().map(|p| (p.id, p.name)).collect();
    let proj = |a: &Value| -> Result<Option<i64>, String> {
        match a.get("project").and_then(Value::as_str) {
            Some(p) => db.project_by_name(p).map(Some).ok_or(format!("'{p}' adında proje yok. Mevcut: {}", names.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join(", "))),
            None => Ok(None),
        }
    };
    let id = || a.get("id").and_then(Value::as_i64).ok_or("id gerekli".to_string());
    match name {
        "list_projects" => Ok(json!(db.projects().iter().map(|p| json!({"id": p.id, "name": p.name, "open_tasks": p.open})).collect::<Vec<_>>())),
        "add_project" => {
            let nm = a["name"].as_str().ok_or("name gerekli")?.trim();
            if db.project_by_name(nm).is_some() { return Err("bu adda proje zaten var".into()); }
            Ok(json!({"id": db.add_project(nm, a["color"].as_i64().unwrap_or(names.len() as i64 % 7) as i32)}))
        }
        "list_tasks" => {
            let done = a["include_done"].as_bool().unwrap_or(false);
            let within = a["due_within_days"].as_i64();
            let pids: Vec<i64> = match proj(a)? { Some(p) => vec![p], None => names.iter().map(|p| p.0).collect() };
            let mut out = vec![];
            for pid in pids {
                for t in db.tasks_of(pid, None) {
                    if t.done && !done { continue; }
                    if let Some(w) = within {
                        match t.due_date { Some(d) if (d - today).num_days() <= w => {}, _ => continue }
                    }
                    out.push(task_json(db, &t, &names));
                }
            }
            Ok(json!({"today": today.to_string(), "tasks": out}))
        }
        "add_task" => {
            let title = a["title"].as_str().ok_or("title gerekli")?;
            let p = parse::parse(&format!("{title} {}", a["due"].as_str().unwrap_or("")), today, now);
            let parent = a["parent_id"].as_i64();
            let pid = match (parent, proj(a)?) {
                (Some(pr), _) => db.task(pr).ok_or("üst görev yok")?.project_id,
                (None, Some(p)) => p,
                (None, None) => match p.project.as_deref().and_then(|x| db.project_by_name(x)) {
                    Some(x) => x,
                    None if names.len() == 1 => names[0].0,
                    None => return Err("project gerekli".into()),
                },
            };
            let tid = db.add_task(pid, parent, p.title.trim(), p.date, p.time, a["high_priority"].as_bool().unwrap_or(p.high));
            Ok(task_json(db, &db.task(tid).unwrap(), &names))
        }
        "update_task" => {
            let id = id()?;
            db.task(id).ok_or("görev yok")?;
            if let Some(t) = a["title"].as_str() { db.rename(id, t.trim()); }
            if let Some(d) = a["due"].as_str() {
                let (dd, tt) = parse::parse_due_only(d, today, now)
                    .or_else(|| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok().map(|x| (Some(x), None)))
                    .ok_or("deadline anlaşılamadı")?;
                db.set_due(id, dd, tt);
            }
            if let Some(h) = a["high_priority"].as_bool() { db.set_high(id, h); }
            Ok(task_json(db, &db.task(id).unwrap(), &names))
        }
        "complete_task" => { let id = id()?; db.task(id).ok_or("görev yok")?; db.set_done(id, a["done"].as_bool().unwrap_or(true)); Ok(json!({"ok": true})) }
        "delete_task" => { let id = id()?; db.task(id).ok_or("görev yok")?; db.delete_task(id); Ok(json!({"ok": true})) }
        _ => Err(format!("bilinmeyen araç: {name}")),
    }
}

pub fn run(db: Db) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = req.get("id").cloned() else { continue }; // bildirim
        let m = req["method"].as_str().unwrap_or("");
        let result = match m {
            "initialize" => json!({"protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}}, "serverInfo": {"name": "yarin-yaparim", "version": env!("CARGO_PKG_VERSION")}}),
            "ping" => json!({}),
            "tools/list" => json!({"tools": tools()}),
            "tools/call" => {
                let p = &req["params"];
                match call(&db, p["name"].as_str().unwrap_or(""), &p["arguments"]) {
                    Ok(v) => json!({"content": [{"type": "text", "text": v.to_string()}]}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                }
            }
            _ => {
                let _ = writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}));
                let _ = out.flush();
                continue;
            }
        };
        let _ = writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "result": result}));
        let _ = out.flush();
    }
}
