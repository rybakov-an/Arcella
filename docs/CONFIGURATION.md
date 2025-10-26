### **CONFIGURATION.md**

# Arcella Configuration System (v0.2.4)

This document describes the logic and behavior of the **current** implementation of the Arcella configuration system, presented in version 0.2.4.

## 1. Base Directory (`base_dir`)

Arcella determines its base directory (`base_dir`) according to the following priority order:

1.  **Binary location check**: If the Arcella binary is located within a `bin` subdirectory, the parent directory of `bin` is used as `base_dir`.
2.  **Local config folder check**: If the directory from which the Arcella binary is run contains a `config` subdirectory, then the run directory itself is used as `base_dir`.
3.  **Fallback (home directory)**: If none of the above conditions are met, the system's home directory is used, with `.arcella` appended (e.g., `~/.arcella` on Unix-like systems, `%USERPROFILE%\.arcella` on Windows).

This logic applies across different operating systems (Linux, AstraLinux, Windows, MacOS).

## 2. Configuration Directory (`config_dir`)

The configuration directory is always located at the path `base_dir.join("config")`. All main configuration files must reside in this directory.

## 3. Main Configuration File (`arcella.toml`)

-   Arcella **creates** the `arcella.toml` file in `config_dir` if it does not exist, **copying it from** `arcella.template.toml`.
-   Arcella **creates** the `arcella.template.toml` file in `config_dir` if it does not exist, **populating it with a built-in template**.
-   Arcella **requires** `arcella.toml` to be **readable**.
-   **It is critically important that the Arcella executable *never* modifies the `arcella.toml` or `arcella.template.toml` files.**

## 4. Loading Composite Configuration (Implementation)

Arcella assembles a single unified configuration object by recursively merging content from the following sources in **increasing priority order**:

1.  **Built-in default configuration**: Values defined in `default_config.toml` (embedded in the executable).
2.  **Recursively loaded files from `includes`**: `.toml` files (excluding `*.template.toml`) **specified in the `includes` array** in `arcella.toml` and other loaded files are processed recursively up to `MAX_CONFIG_DEPTH` (inclusive). The order of files is determined by the lexicographical order of filenames and paths, as well as the order they are listed in `includes`.

**Merging Order (from lowest to highest priority):**
-   First, the **built-in default configuration** is loaded.
-   Then, **recursively loaded and merged** files found via `includes` are processed.
-   Finally, the **`arcella.toml`** is applied.

**The `#redef` Mechanism:**
-   If a key in a configuration file **ends with `#redef`** (e.g., `arcella.log.level#redef = "debug"`), the **value for the original key** (`arcella.log.level`) **can be overridden** by configuration layers from `includes` *below* the current one.
-   This allows the `arcella.toml` file or the built-in default config to **firmly fix** certain values, preventing them from being changed via `includes`.
-   If a subsequent configuration layer **attempts** to set a value for a key that was fixed, a **warning** (`ValueError`) is **generated**, and the **original value is preserved**.

**Built-in Default Configuration**
-   Sets default values for all configuration parameters.
-   The `includes` section in the **built-in default configuration** specifies only the `arcella.toml` file.
-   Files loaded via `includes` can only supplement parameters **from the built-in default configuration** within the `arcella.custom` and `arcella.modules` sections.

**In summary:** The resulting configuration object is built such that the **built-in default configuration** defines base values, which can be *supplemented or overridden* (if `#redef` is used) by values from files specified in `includes`, and **finally refined** by the content of `arcella.toml`.

## 5. Configuration Integrity Check

-   On startup, Arcella **initializes** the `IntegrityChecker`, passing it a list of files to monitor.
-   In the **current implementation**, the list of files for integrity checking (`integrity_check_paths`) in `ArcellaConfig` is **initialized** with only the `arcella.toml` file.
-   This means that **configuration file integrity checking** in the current version is implemented only with respect to the main configuration file.
-   In the future, `integrity_check_paths` could be **populated** with paths to files loaded via `includes`.

## 6. StorageManager and Directories

-   `StorageManager` **does not** create or **modify permissions** for `base_dir`, `config_dir`, and `log`. It **only** checks their correctness (e.g., readability, write access to subdirectories).
-   `StorageManager` **creates** the following subdirectories inside `base_dir` if they do not exist:
    -   `cache_dir`
    -   `modules_dir`

## 7. Initialization Sequence (Implementation)

Configuration loading and directory setup occur in a strict sequence:

1.  **Determine `base_dir`**: `fs_utils::find_base_dir()` is called.
2.  **Determine `config_dir`**: `base_dir.join("config")`.
3.  **Ensure `config_dir` and `arcella.toml`/`arcella.template.toml` files exist**: `ensure_config_template(&config_dir)` is called.
4.  **Load configs**: `arcella_fs_utils::load_config_recursive_from_content` is called with `DEFAULT_CONFIG_CONTENT`, `arcella.toml`, and `includes`.
5.  **Merge layers considering `#redef`**: The logic for merging `configs` into `final_values` is executed.
6.  **Extract, sort, and index parameters**: The `arcella_types::value::ConfigData` object is created.
7.  **Create `ArcellaConfig`**: The `ArcellaConfig` structure is created and returned with the extracted key values.
8.  **Initialize logging**: The logging system is initialized using `ArcellaConfig` and creates `log_dir`.
9.  **Create other directories**: `StorageManager` creates `cache_dir` and `modules_dir` based on `ArcellaConfig`.

## 7. Logging

-   Before the main logging system starts, all errors and warnings are collected in `warnings`.
-   After the main logging system starts, all events from `warnings` are passed to the main logging logic.
-   If a critical error occurs during the startup of the main logging system or during configuration building, `warnings` is dumped to the console and an `init.log` file.

## 8. Override Warnings

If the configuration loading process detects an attempt to override a key-value pair that was already defined by a source of higher priority (specifically, if a key in an earlier layer does not have `#redef`), Arcella **emits a warning** (`ConfigLoadWarning::ValueError`) which is **collected** in the `warnings` vector. The value from the higher priority source is preserved.

---

### 📄 **Configuration File Example**

#### 1. `default_config.toml` (built-in, layer 0)

```toml
[arcella.log]
level = "info"
file = "arcella_default.log"

[arcella.server]
port = 8080
host = "0.0.0.0"

includes = ["arcella.toml"]
```

#### 2. `arcella.toml`

```toml
[arcella.log]
level#redef = "warn" # Sets "warn" and marks that "level" can be overridden
file = "arcella_main.log" # Overrides "arcella_default.log" from default_config.toml

[arcella.server]
port = 9000 # Overrides 8080 from default_config.toml
# arcella.server.host remains "0.0.0.0" from arcella_default.log
```

#### 2. `level_1.toml` (from `includes`, layer 1)

```toml
[arcella.log]
level = "debug" # Overrides "warn" from arcella.toml and "info" from default_config.toml

[arcella.server]
host = "127.0.0.1" # Cannot override "0.0.0.0" from default_config.toml because `arcella.toml` does not have the #redef key
name = "www.server.net" # Cannot be applied because it does not exist in default_config.toml and is not in the `arcella.custom` or `arcella.modules` sections

[arcella.custom]
message = "This is an additional parameter" # Cannot override "0.0.0.0" from default_config.toml because `arcella.toml` does not have the #redef key
```

#### Resulting Configuration

```toml
[arcella.log]
level = "debug"
file = "arcella_main.log"

[arcella.server]
port = 9000
host = "0.0.0.0"

[arcella.custom]
message = "This is an additional parameter"
```