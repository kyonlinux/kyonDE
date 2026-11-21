use std::{fs, path::PathBuf};

use kyon_ipc::{Bezier, KeyBind, KyonConfig};
use mlua::{Lua, LuaSerdeExt, Table, Value};
use serde_json::Value as Json;
use tracing::{error, info, warn};

#[derive(Default, Clone)]
struct LuaCollector {
    patches: Json,
    binds: Vec<KeyBind>,
    exec: Vec<String>,
    env: Vec<kyon_ipc::EnvVar>,
    outputs: Vec<Json>,
    rules: Vec<Json>,
}

pub fn ensure_config() -> PathBuf {
    let dir = kyon_ipc::config_home();
    let path = kyon_ipc::config_path();
    if let Err(err) = fs::create_dir_all(&dir) {
        warn!(?err, "failed to create config directory");
    }
    if !path.exists() {
        if let Err(err) = fs::write(&path, kyon_ipc::default_config_lua()) {
            warn!(?err, "failed to write default config");
        } else {
            info!(path = %path.display(), "created default config");
        }
    }
    path
}

pub fn load() -> KyonConfig {
    let path = ensure_config();
    let mut config = KyonConfig::default();
    match load_lua(&path) {
        Ok((patches, binds, exec, outputs, rules, env)) => {
            if !patches.is_null() {
                apply_json_patch(&mut config, &patches);
            }
            config.env.extend(env);
            if !binds.is_empty() {
                config.binds = merge_binds(config.binds, binds);
            }
            config.exec.extend(exec);
            for output in outputs {
                match serde_json::from_value(output) {
                    Ok(rule) => config.outputs.push(rule),
                    Err(err) => warn!(?err, "invalid output rule in config"),
                }
            }
            for rule in rules {
                match serde_json::from_value(rule) {
                    Ok(rule) => config.rules.push(rule),
                    Err(err) => warn!(?err, "invalid window rule in config"),
                }
            }
        }
        Err(err) => {
            error!(?err, path = %path.display(), "failed to load lua config");
        }
    }
    apply_state_file(&mut config);
    normalize(&mut config);
    config
}

fn load_lua(path: &PathBuf) -> Result<(Json, Vec<KeyBind>, Vec<String>, Vec<Json>, Vec<Json>, Vec<kyon_ipc::EnvVar>), mlua::Error> {
    let source = fs::read_to_string(path)?;
    let lua = Lua::new();
    let kyde = lua.create_table()?;
    let collector = std::rc::Rc::new(std::cell::RefCell::new(LuaCollector::default()));

    for section in ["decor", "blur", "layout", "input", "shell", "gpu"] {
        let c = collector.clone();
        let name = section.to_string();
        let func = lua.create_function(move |lua, value: Table| {
            let json: Json = lua.from_value(Value::Table(value))?;
            let mut state = c.borrow_mut();
            let wrapped = serde_json::json!({ name.clone(): json });
            merge_json(&mut state.patches, wrapped);
            Ok(())
        })?;
        kyde.set(section, func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |lua, value: Table| {
            let json: Json = lua.from_value(Value::Table(value))?;
            let mut state = c.borrow_mut();
            merge_json(&mut state.patches, serde_json::json!({ "animations": json }));
            Ok(())
        })?;
        kyde.set("animations", func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |lua, value: Value| {
            let json: Json = match value {
                Value::String(s) => {
                    let path = s.to_str()?.to_string();
                    serde_json::json!({ "style": "image", "path": path })
                }
                Value::Table(t) => lua.from_value(Value::Table(t))?,
                _ => return Err(mlua::Error::runtime("wallpaper expects a string or table")),
            };
            let mut state = c.borrow_mut();
            merge_json(&mut state.patches, serde_json::json!({ "wallpaper": json }));
            Ok(())
        })?;
        kyde.set("wallpaper", func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |lua, value: Value| {
            let json: Json = match value {
                Value::String(s) => serde_json::json!({ "theme": s.to_str()?.to_string() }),
                Value::Table(t) => lua.from_value(Value::Table(t))?,
                _ => return Err(mlua::Error::runtime("theme expects a name or table")),
            };
            let mut state = c.borrow_mut();
            let merged = serde_json::json!({ "shell": json });
            merge_json(&mut state.patches, merged);
            Ok(())
        })?;
        kyde.set("theme", func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |_lua, (a, b, c_arg): (Value, Option<Value>, Option<Value>)| {
            let (mods, key, action) = match (a, b, c_arg) {
                (Value::String(mods), Some(Value::String(key)), Some(Value::String(action))) => (
                    mods.to_str()?.to_string(),
                    key.to_str()?.to_string(),
                    action.to_str()?.to_string(),
                ),
                (Value::String(combo), Some(Value::String(action)), None) => {
                    let (mods, key) = split_combo(&combo.to_str()?);
                    (mods, key, action.to_str()?.to_string())
                }
                _ => {
                    return Err(mlua::Error::runtime(
                        "bind expects (mods, key, action) or (combo, action)",
                    ));
                }
            };
            c.borrow_mut().binds.push(KeyBind { mods, key, action });
            Ok(())
        })?;
        kyde.set("bind", func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |_lua, command: String| {
            c.borrow_mut().exec.push(command);
            Ok(())
        })?;
        kyde.set("exec", func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |_lua, (key, value): (String, String)| {
            c.borrow_mut().env.push(kyon_ipc::EnvVar { key, value });
            Ok(())
        })?;
        kyde.set("env", func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |lua, value: Table| {
            let json: Json = lua.from_value(Value::Table(value))?;
            c.borrow_mut().outputs.push(json);
            Ok(())
        })?;
        kyde.set("output", func)?;
    }

    {
        let c = collector.clone();
        let func = lua.create_function(move |lua, value: Table| {
            let json: Json = lua.from_value(Value::Table(value))?;
            c.borrow_mut().rules.push(json);
            Ok(())
        })?;
        kyde.set("rule", func)?;
    }

    {
        let func = lua.create_function(move |lua, path: String| {
            let source = fs::read_to_string(&path)
                .map_err(|err| mlua::Error::runtime(format!("failed to read {path}: {err}")))?;
            lua.load(&source).set_name(&path).exec()
        })?;
        kyde.set("include", func)?;
    }

    lua.globals().set("kyde", kyde)?;
    lua.load(&source).set_name(path.to_string_lossy()).exec()?;

    let state = collector.borrow().clone();
    Ok((state.patches, state.binds, state.exec, state.outputs, state.rules, state.env))
}

fn split_combo(combo: &str) -> (String, String) {
    let mut parts: Vec<&str> = combo.split('+').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return (String::new(), String::new());
    }
    let key = parts.pop().unwrap_or_default().to_string();
    (parts.join("+"), key)
}

pub fn merge_binds(mut base: Vec<KeyBind>, adds: Vec<KeyBind>) -> Vec<KeyBind> {
    for add in adds {
        if let Some(existing) = base
            .iter_mut()
            .find(|bind| bind.mods.eq_ignore_ascii_case(&add.mods) && bind.key.eq_ignore_ascii_case(&add.key))
        {
            existing.action = add.action;
        } else {
            base.push(add);
        }
    }
    base
}

pub fn load_state_overrides() -> Vec<Json> {
    let path = kyon_ipc::state_path();
    let Ok(source) = fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Json>(&source) {
        Ok(value) if value.is_object() => vec![value],
        Ok(_) => {
            warn!("state.json must contain an object");
            Vec::new()
        }
        Err(err) => {
            warn!(?err, "failed to parse state.json");
            Vec::new()
        }
    }
}

fn apply_state_file(config: &mut KyonConfig) {
    for patch in load_state_overrides() {
        apply_json_patch(config, &patch);
    }
}

pub fn save_state(config: &KyonConfig) {
    let dir = kyon_ipc::config_home();
    if let Err(err) = fs::create_dir_all(&dir) {
        warn!(?err, "failed to create state directory");
        return;
    }
    let value = serde_json::json!({
        "decor": config.decor,
        "blur": config.blur,
        "animations": config.animations,
        "layout": config.layout,
        "gpu": config.gpu,
        "input": config.input,
        "shell": config.shell,
        "wallpaper": config.wallpaper,
    });
    if let Err(err) = fs::write(
        kyon_ipc::state_path(),
        serde_json::to_vec_pretty(&value).unwrap_or_default(),
    ) {
        warn!(?err, "failed to save state.json");
    }
}

pub fn apply_json_patch(config: &mut KyonConfig, patch: &Json) {
    let Some(object) = patch.as_object() else {
        return;
    };
    for (key, value) in object {
        apply_field(config, key, value);
    }
}

fn apply_field(config: &mut KyonConfig, key: &str, value: &Json) {
    macro_rules! replace {
        ($field:ident, $ty:ty) => {
            match serde_json::from_value::<$ty>(value.clone()) {
                Ok(parsed) => config.$field = parsed,
                Err(err) => warn!(?err, field = stringify!($field), "invalid config value"),
            }
        };
    }
    match key {
        "decor" => replace!(decor, kyon_ipc::Decor),
        "blur" => replace!(blur, kyon_ipc::Blur),
        "animations" => replace!(animations, kyon_ipc::Animations),
        "layout" => replace!(layout, kyon_ipc::Layout),
        "gpu" => replace!(gpu, kyon_ipc::Gpu),
        "input" => replace!(input, kyon_ipc::Input),
        "shell" => replace!(shell, kyon_ipc::Shell),
        "wallpaper" => replace!(wallpaper, kyon_ipc::Wallpaper),
        "binds" => {
            if let Ok(parsed) = serde_json::from_value::<Vec<KeyBind>>(value.clone()) {
                config.binds = merge_binds(config.binds.clone(), parsed);
            }
        }
        "exec" => {
            if let Ok(parsed) = serde_json::from_value::<Vec<String>>(value.clone()) {
                config.exec.extend(parsed);
            }
        }
        "outputs" => replace!(outputs, Vec<kyon_ipc::OutputRule>),
        "rules" => replace!(rules, Vec<kyon_ipc::WindowRule>),
        _ => warn!(field = key, "unknown config field"),
    }
}

fn normalize(config: &mut KyonConfig) {
    config.decor.rounding = config.decor.rounding.clamp(0, 48);
    config.decor.gaps_in = config.decor.gaps_in.clamp(0, 64);
    config.decor.gaps_out = config.decor.gaps_out.clamp(0, 96);
    config.decor.border_width = config.decor.border_width.clamp(0, 12);
    config.decor.titlebar.height = config.decor.titlebar.height.clamp(18, 72);
    config.decor.titlebar.font_size = config.decor.titlebar.font_size.clamp(8.0, 32.0);
    config.decor.active_opacity = config.decor.active_opacity.clamp(0.1, 1.0);
    config.decor.inactive_opacity = config.decor.inactive_opacity.clamp(0.1, 1.0);
    config.decor.shadow.size = config.decor.shadow.size.clamp(0, 80);
    config.decor.shadow.opacity = config.decor.shadow.opacity.clamp(0.0, 1.0);
    config.blur.size = config.blur.size.clamp(0.0, 40.0);
    config.blur.passes = config.blur.passes.clamp(1, 6);
    config.blur.opacity = config.blur.opacity.clamp(0.0, 1.0);
    config.animations.speed = config.animations.speed.clamp(1.0, 10.0);
    if let Bezier::Custom(points) = &mut config.animations.bezier {
        for point in points.iter_mut() {
            *point = point.clamp(0.0, 1.0);
        }
    }
    config.layout.master_ratio = config.layout.master_ratio.clamp(0.2, 0.85);
    config.layout.scrolling.column_width = config.layout.scrolling.column_width.clamp(0.25, 1.0);
    config.shell.panel_height = config.shell.panel_height.clamp(24, 96);
    config.shell.panel_opacity = config.shell.panel_opacity.clamp(0.0, 1.0);
    config.shell.workspaces = config.shell.workspaces.clamp(1, 9);
    if !matches!(config.layout.mode.as_str(), "tiling" | "floating" | "scrolling") {
        config.layout.mode = "tiling".into();
    }
}

pub fn snapshot_config(config: &KyonConfig) -> Json {
    serde_json::to_value(config).unwrap_or(Json::Null)
}

fn merge_json(base: &mut Json, patch: Json) {
    match (base, patch) {
        (Json::Object(base_map), Json::Object(patch_map)) => {
            for (key, value) in patch_map {
                if value.is_null() {
                    continue;
                }
                match base_map.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (base_slot, patch_value) => {
            if !patch_value.is_null() {
                *base_slot = patch_value;
            }
        }
    }
}
