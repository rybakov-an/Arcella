### **CONFIGURATION.md**

# Arcella Configuration System

This document describes the logic and behavior of the Arcella configuration system introduced in version 0.2.4.

## 1. Base Directory (`base_dir`)

Arcella determines its base directory (`base_dir`) according to the following priority order:

1.  **Binary Location Check**: If the Arcella binary is located in a subdirectory named `bin`, the parent directory of `bin` is used as `base_dir`.
2.  **Local Config Check**: If the directory from which the Arcella binary was executed contains a subdirectory named `config`, then the execution directory itself is used as `base_dir`.
3.  **User Home Fallback**: If neither of the above conditions are met, the system's user home directory is used, appended with `.arcella` (e.g., `~/.arcella` on Unix-like systems, `%USERPROFILE%\.arcella` on Windows).

This logic applies across different operating systems (Linux, AstraLinux, Windows, MacOS).

## 2. Configuration Directory (`config_dir`)

The configuration directory is always located at `base_dir.join("config")`. All primary configuration files must reside within this directory.

## 3. Main Configuration File (`arcella.toml`)

-   Arcella requires a main configuration file named `arcella.toml` to be present in `config_dir`.
-   If `arcella.toml` is not found, Arcella will terminate with an error.
-   If the `config_template` subdirectory does not exist within `config_dir`, Arcella will create it and generate a default template file named `arcella.template.toml` inside it. This template serves as an example for users.
-   **Crucially, the Arcella executable itself will *never* modify the `arcella.toml` file.**

## 4. Loading Composite Configuration (refined order)

Arcella assembles a single unified configuration object by recursively merging content from multiple sources. **Sources with higher priority supplement, but do *not* override, values from lower-priority sources.** If a value is already set by a higher-priority source, it is **preserved**, and any attempt to redefine it is logged as a warning.

The order *from lowest to highest priority* (where later steps have a stronger influence on the result) is:

1.  **Contents of subdirectories listed in `includes` (depth 1):**
    -   First, consider **all** `.toml` files (excluding `*.template.toml`) in the **subdirectories** listed in the `includes` array in `arcella.toml`.
    -   Then, **recursively** process files in subdirectories *of those* subdirectories (depth 2).
    -   Then, **recursively** process files in subdirectories *of the previous* level (depth 3).
    -   **Rule:** Within each level (depth), files are processed in **lexicographical order** of their names. Within a single file, values are processed in the order they appear.
    -   **Note:** Processing of nested `.toml` files *referenced inside* files from `includes` is not performed. Only files directly located in the specified subdirectories, up to 3 levels deep, are considered.

2.  **Nested `.toml` files listed in `includes`:**
    -   Then, consider the **`.toml` files explicitly listed** in the `includes` array in `arcella.toml`.
    -   **Rule:** Files are processed in **lexicographical order** of their names within the `includes` array. Within a single file, values are processed in the order they appear.

3.  **Main configuration file (`arcella.toml`):**
    -   Finally, the content of `arcella.toml` is merged with the already assembled result.
    -   **Rule:** Values from `arcella.toml` have the **highest priority**. Any keys defined here will *not be overridden* by values from previous sources, but may *supplement* them.

**Result:** The resulting configuration object is built such that `arcella.toml` defines base and final values, which can be *supplemented* (but not overwritten back) by values from files and subdirectories specified in `includes`, taking into account nesting depth and lexicographical order.

## 5. Configuration Integrity Check

-   Upon startup, Arcella performs a **one-time** integrity check on the base configuration files (`arcella.toml` and any files explicitly included from `config_dir` as per `arcella.toml`).
-   This check verifies if these files have been modified since Arcella started (e.g., by comparing file modification times or checksums).
-   If any modification is detected, Arcella will terminate immediately with a security error message. This prevents runtime tampering with the core configuration.
-   `arcella.toml` can optionally specify *additional* files or directories to be included in this integrity check.

## 6. Storage Manager and Directories

-   The `StorageManager` does **not** create or modify the permissions of `base_dir` or `config_dir`. It **only** checks their correctness (e.g., readability, writability for subdirectories).
-   The `StorageManager` **creates** the following subdirectories within `base_dir` if they do not exist:
    -   `log`
    -   `cache`
    -   `modules`

## 7. Initialization Sequence

Configuration loading and directory setup follow a strict sequence:

1.  **Load Configs**: All configuration files are read and merged according to the rules described above.
2.  **Create Log Directory**: The `log` subdirectory within `base_dir` is created.
3.  **Initialize Logging**: The logging system is initialized using the loaded configuration.
4.  **Create Other Directories**: The `cache` and `modules` subdirectories are created.

## 8. Override Warnings

If the configuration loading process detects an attempt to redefine a key-value pair that was already defined by a higher-priority source, Arcella will issue a **warning** message via the logging system. The value from the higher-priority source is retained.