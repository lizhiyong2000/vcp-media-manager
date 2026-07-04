# vcp-media-manager

Web 管理后端（BFF），管理 **设备** 与多 **region / media-server** 实例，为 [vcp-media-server](../vcp-media-server) 提供统一 REST API。

## 工程关系

```
vcp/
├── vcp-media-server/    # Rust 媒体核心（可多实例、按 region 部署）
├── vcp-media-manager/ # 本工程：设备管理 + 多节点聚合
└── vcp-media-web/     # Web 管理前端
```

## 核心概念

| 概念 | 说明 |
|------|------|
| **Region** | 逻辑区域，其下挂多台 media-server |
| **Media Server** | 具体流媒体节点，负责实际推/拉/播 |
| **Device** | manager 管理的主体；**设备 ID 通常等于推流 stream_id** |
| **流关联** | 设备向所属 media-server 推流时，manager 按 stream_id 匹配设备并展示在线状态 |

## 快速开始

```bash
cd vcp-media-manager
cp .env.example .env
cp servers.json.example servers.json   # 可按需增删 region / 节点
cargo run
```

默认监听 `http://127.0.0.1:8090`。请先启动至少一台 media-server：

```bash
cd ../vcp-media-server
cargo run --bin vcp-media-server
```

## 配置

### 多 region / 多节点 — `servers.json`

```json
{
  "regions": [{ "id": "cn-east", "name": "华东" }],
  "servers": [{
    "id": "cn-east-01",
    "regionId": "cn-east",
    "name": "华东节点 1",
    "apiUrl": "http://127.0.0.1:8081",
    "publicHost": "127.0.0.1",
    "rtmpPort": 1935,
    "rtspPort": 554
  }]
}
```

`httpPort` 由 `apiUrl` 自动解析；HLS / FLV / WebRTC 测试页与 HTTP API 同端口。

未提供 `servers.json` 时，回退到 `.env` 中的单节点配置（`MEDIA_SERVER_URL`、`MEDIA_PUBLIC_HOST`）。

### 设备持久化

设备列表写入 `data/devices.json`（路径由 `DEVICES_FILE` 指定）。

## 主要 API

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | 管理后端 + 各 media-server 健康状态 |
| GET | `/api/regions` | 区域列表 |
| GET | `/api/servers` | 媒体节点列表（含状态、流/设备数） |
| GET | `/api/servers/:id` | 媒体节点详情（流、绑定设备、指标） |
| GET | `/api/devices` | 设备列表（含推流在线状态） |
| POST | `/api/devices` | 创建设备 |
| GET | `/api/devices/:id` | 设备详情 |
| PUT | `/api/devices/:id` | 更新设备 |
| DELETE | `/api/devices/:id` | 删除设备 |
| GET | `/api/devices/:id/play-urls` | 设备播放地址（按所属节点生成） |

仍保留 `/api/streams` 等流级接口，创建设备后推流会自动关联。

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `PORT` | `8090` | 管理后端端口 |
| `SERVERS_CONFIG` | `servers.json` | region / 节点注册表 |
| `DEVICES_FILE` | `data/devices.json` | 设备数据文件 |
| `MEDIA_SERVER_URL` | `http://127.0.0.1:8081` | 无 `servers.json` 时的上游 API |
| `MEDIA_PUBLIC_HOST` | `127.0.0.1` | 无 `servers.json` 时生成播放 URL 的主机名 |

## 生产部署

```bash
cargo build --release
./target/release/vcp-media-manager
```

建议与 `vcp-media-web` 同机部署，由 Nginx 反代 `/api` 到本服务。
