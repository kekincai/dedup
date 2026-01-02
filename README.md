# dedup - 跨平台文件索引与查重工具

一个用 Rust 编写的命令行工具，用于高效地查找重复文件。

## 功能特点

- 🚀 **分阶段哈希**：先用 fast hash（前后 64KB）快速筛选，再用 full hash 精确判定
- 📦 **SQLite 存储**：索引结果持久化，支持增量扫描
- 🔄 **增量更新**：只处理变化的文件，第二次扫描极快
- 🖥️ **跨平台**：支持 Windows / macOS / Linux
- 📁 **文件夹选择**：不指定路径时自动弹出系统对话框

## 安装

### 从源码编译

```bash
# 克隆项目
git clone https://github.com/kejincai/dedup.git
cd dedup

# 编译 release 版本
cargo build --release

# 可执行文件在 target/release/dedup
```

### 从 Release 下载

前往 [Releases](https://github.com/kejincai/dedup/releases) 下载对应平台的预编译版本。

## 使用方法

### 方式一：双击运行（弹出文件夹选择）

直接双击 `dedup` 可执行文件，会弹出文件夹选择对话框。

> ⚠️ 注意：双击运行默认执行 `--help`，需要在终端中使用完整命令。

### 方式二：终端命令行（推荐）

#### 1. 建立索引

```bash
# 指定文件夹
./dedup index /path/to/folder --fast-hash --full-hash

# 不指定路径，弹出选择对话框
./dedup index --fast-hash --full-hash

# 只计算 fast hash（更快）
./dedup index /path/to/folder --fast-hash
```

#### 2. 增量更新

```bash
# 只更新变化的文件
./dedup update /path/to/folder --fast-hash --full-hash
```

#### 3. 查看重复文件

```bash
# 按完整哈希查重（精确匹配）
./dedup dup --by full

# 按快速哈希查重（疑似重复）
./dedup dup --by fast

# 只显示大于 10MB 的重复文件
./dedup dup --by full --min-size 10MB
```

#### 4. 导出结果

```bash
# 导出为 CSV
./dedup export --by full --csv duplicates.csv

# 导出为 JSON
./dedup export --by full --json duplicates.json

# 同时导出
./dedup export --by full --csv duplicates.csv --json duplicates.json
```

#### 5. 查看统计信息

```bash
./dedup stats
```

输出示例：
```
数据库统计信息:
  文件总数: 1234
  总大小: 5.6 GB
  已计算 fast_hash: 1234
  已计算 full_hash: 1234
  疑似重复组数 (fast): 56
  精确重复组数 (full): 23
```

## 命令参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--db <path>` | 数据库文件路径 | `dedup.db` |
| `--fast-hash` | 计算快速哈希（前后 64KB） | 否 |
| `--full-hash` | 计算完整哈希 | 否 |
| `--workers <n>` | 并行线程数 | 4 |
| `--by <mode>` | 查重模式：`full` 或 `fast` | `full` |
| `--min-size <size>` | 最小文件大小（如 `10MB`） | 0 |

## 典型工作流程

```bash
# 1. 首次扫描，建立完整索引
./dedup index ~/Photos --fast-hash --full-hash

# 2. 查看重复文件
./dedup dup --by full

# 3. 导出结果供人工确认
./dedup export --by full --csv ~/Desktop/duplicates.csv

# 4. 之后定期增量更新
./dedup update ~/Photos --fast-hash --full-hash
```

## 输出格式

### CSV 格式

```csv
duplicate_group_id,path,size,hash,source_id
1,/path/to/file1.jpg,1048576,abc123...,dev:12345
1,/path/to/file2.jpg,1048576,abc123...,dev:12345
2,/path/to/file3.mp4,52428800,def456...,dev:12345
2,/path/to/file4.mp4,52428800,def456...,dev:12345
```

### JSON 格式

```json
{
  "groups": [
    {
      "group_id": 1,
      "size": 1048576,
      "hash": "abc123...",
      "file_count": 2,
      "files": [
        {"path": "/path/to/file1.jpg", "source_id": "dev:12345"},
        {"path": "/path/to/file2.jpg", "source_id": "dev:12345"}
      ]
    }
  ],
  "total_groups": 1,
  "total_duplicate_files": 2,
  "total_wasted_space": 1048576
}
```

## 注意事项

- ⚠️ **不会自动删除文件**，只提供查重报告
- 📂 支持扫描 SMB 网络共享（挂载后的路径）
- 💾 数据库文件可长期保留，支持增量更新
- 🔒 文件不可读时会跳过并记录警告

## License

MIT
