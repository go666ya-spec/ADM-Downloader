import { readFileSync, writeFileSync } from 'node:fs'

function patch(file, pairs) {
  let text = readFileSync(file, 'utf8')
  for (const [from, to] of pairs) {
    if (!text.includes(from)) {
      console.log(`MISS ${file}: ${from.slice(0, 60)}`)
      continue
    }
    text = text.split(from).join(to)
  }
  writeFileSync(file, text)
}

patch('src-tauri/src/queue.rs', [
  [
    `use std::path::PathBuf;`,
    `use std::collections::HashMap;
use std::path::PathBuf;`,
  ],
  [
    `    /// 持久化文件位置（queue.json）
    store_path: PathBuf,`,
    `    /// 持久化文件位置（queue.json）
    store_path: PathBuf,
    /// 上一次推送的状态，用于只推变化的条目
    last_emit: Mutex<HashMap<u64, String>>,`,
  ],
  [
    `            keeping_awake: std::sync::atomic::AtomicBool::new(false),
            store_path,`,
    `            keeping_awake: std::sync::atomic::AtomicBool::new(false),
            store_path,
            last_emit: Mutex::new(HashMap::new()),`,
  ],
  [
    `pub struct TaskSnapshot {`,
    `/// 结构变化的增量：只包含变化的条目与被删除的 id
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    pub upserts: Vec<TaskSnapshot>,
    pub removed: Vec<u64>,
}

pub struct TaskSnapshot {`,
  ],
  [
    `    fn emit_changed(&self) {
        self.persist();
        let snapshot = self.snapshot();
        let _ = self.app.emit("tasks:changed", snapshot);
    }`,
    `    fn emit_changed(&self) {
        self.persist();

        let snapshot = self.snapshot();
        let mut last = self.last_emit.lock().unwrap();

        let mut upserts: Vec<TaskSnapshot> = Vec::new();
        let mut current: HashMap<u64, String> = HashMap::with_capacity(snapshot.len());

        for task in &snapshot {
            let encoded = serde_json::to_string(task).unwrap_or_default();
            if last.get(&task.id) != Some(&encoded) {
                upserts.push(task.clone());
            }
            current.insert(task.id, encoded);
        }

        let removed: Vec<u64> = last
            .keys()
            .copied()
            .filter(|id| !current.contains_key(id))
            .collect();

        *last = current;
        drop(last);

        /* 没有变化就不推 */
        if upserts.is_empty() && removed.is_empty() {
            return;
        }

        let _ = self
            .app
            .emit("tasks:changed", TaskPatch { upserts, removed });
    }`,
  ],
])

console.log('task patch emit patched')
