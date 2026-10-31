## 游戏状态：服务器发来的配置和眼下的情况，加上查名称、按格子查东西的索引。
## 服务器说了算：这里不算规则，只存服务器给的东西。
extends Node

signal logged_in
signal view_changed
signal waiting_changed
signal turn_ended(report: Dictionary)
signal command_result(req: int, ok: bool, ids: Array, error: String, desc: String)
signal march_confirmation(command: Dictionary, cities: Array, desc: String)
signal incompatible_server(message: String)
signal server_error(message: String)

var me := -1
var config := {}
var view := {}
var last_turn = null
## 点了结束回合的人、还在等的人
var ready_ids: Array = []
var waiting_ids: Array = []

# ---- 由 view 建的索引，键都是 Vector2i 或 int
var cells := {}           # 格子 → CellReport
var vision := {}          # 视野里的格子 → true
var my_cities := {}       # id → CityReport（自己的城，全部内情）
var foreign_cities := {}  # id → ForeignCity（见过的外观）
var tile_city := {}       # 格子 → 城 id（只含知道归属的）
var seat_city := {}       # 治所格子 → 城 id
var armies_at := {}       # 格子 → [{own, data}]
var my_armies := {}       # id → ArmyReport
var foreign_armies := {}  # id → ForeignArmy
var characters := {}      # id → 人物（含自己）
var countries := {}       # id → Country

var _req := 0
var _pending_commands := {}
var _pending := {}  # req → 指令的说明


func _ready() -> void:
	Net.message.connect(_on_message)


func login(player: int) -> void:
	Net.send({"type": "login", "player": player})


## 下指令，返回 req。`desc` 是给日志看的说明
func send_command(cmd: Dictionary, desc := "") -> int:
	_req += 1
	_pending_commands[_req] = cmd.duplicate(true)
	_pending[_req] = desc if desc != "" else str(cmd.get("type", ""))
	Net.send({"type": "command", "req": _req, "command": _ints(cmd)})
	return _req


## Godot 解析 JSON 时数字全是浮点数，服务器的 id 要整数：整数值的浮点数一律转成整数
static func _ints(v):
	if v is float and is_equal_approx(v, roundf(v)) and absf(v) < 9.0e15:
		return int(v)
	if v is Dictionary:
		var out := {}
		for k in v:
			out[k] = _ints(v[k])
		return out
	if v is Array:
		return v.map(func(x): return _ints(x))
	return v


func request_view() -> void:
	Net.send({"type": "view"})


func end_turn(ready: bool) -> void:
	Net.send({"type": "end_turn", "ready": ready})


func _on_message(msg: Dictionary) -> void:
	match str(msg.get("type", "")):
		"welcome":
			# 新界面不能使用缺少行业配置的旧协议，避免随后反复访问不存在的字段。
			var server_config: Dictionary = msg.get("config", {})
			var industries = server_config.get("industries")
			if not industries is Array or industries.is_empty():
				incompatible_server.emit("服务器未提供行业配置，请停止旧服务器，重新运行 cargo run -p zhou-server 后再连接。")
				return
			me = int(msg.player)
			config = msg.config
			last_turn = msg.get("last_turn")
			_set_view(msg.view)
			logged_in.emit()
		"view":
			_set_view(msg.view)
		"turn_ended":
			ready_ids = []
			waiting_ids = []
			_set_view(msg.report)
			turn_ended.emit(msg.report)
			waiting_changed.emit()
		"waiting":
			ready_ids = msg.ready.map(func(x): return int(x))
			waiting_ids = msg.waiting.map(func(x): return int(x))
			waiting_changed.emit()
		"march_confirmation":
			var req := int(msg.req)
			var command: Dictionary = _pending_commands.get(req, {})
			var desc: String = _pending.get(req, "行军")
			_pending_commands.erase(req)
			_pending.erase(req)
			march_confirmation.emit(command, msg.cities, desc)
		"result":
			var req := int(msg.req)
			var desc: String = _pending.get(req, "")
			_pending.erase(req)
			_pending_commands.erase(req)
			var err = msg.get("error")
			command_result.emit(req, bool(msg.ok), msg.get("ids", []), "" if err == null else str(err), desc)
		"error":
			server_error.emit(str(msg.get("message", "")))


func _set_view(v: Dictionary) -> void:
	view = v
	cells.clear()
	vision.clear()
	my_cities.clear()
	foreign_cities.clear()
	tile_city.clear()
	seat_city.clear()
	armies_at.clear()
	my_armies.clear()
	foreign_armies.clear()
	characters.clear()
	countries.clear()
	for c in v.map:
		var h := hx(c.pos)
		cells[h] = c
		if c.city != null:
			tile_city[h] = int(c.city)
	for p in v.vision:
		vision[hx(p)] = true
	for c in v.cities:
		my_cities[int(c.id)] = c
		seat_city[hx(c.seat)] = int(c.id)
		for t in c.tiles:
			tile_city[hx(t.pos)] = int(c.id)
	for c in v.foreign_cities:
		foreign_cities[int(c.id)] = c
		seat_city[hx(c.seat)] = int(c.id)
		for t in c.tiles:
			tile_city[hx(t)] = int(c.id)
	for a in v.armies:
		my_armies[int(a.id)] = a
		_add_army(hx(a.pos), true, a)
	for a in v.foreign_armies:
		foreign_armies[int(a.id)] = a
		_add_army(hx(a.pos), false, a)
	for c in v.characters:
		characters[int(c.id)] = c
	if v.character != null:
		characters[int(v.character.id)] = v.character
	for c in v.countries:
		countries[int(c.id)] = c
	view_changed.emit()


func _add_army(h: Vector2i, own: bool, data: Dictionary) -> void:
	if not armies_at.has(h):
		armies_at[h] = []
	armies_at[h].append({"own": own, "data": data})


# ---------------------------------------------------------------- 坐标

static func hx(d) -> Vector2i:
	return Vector2i(int(d.q), int(d.r))


static func hd(h: Vector2i) -> Dictionary:
	return {"q": h.x, "r": h.y}


## 显示用的坐标：和服务器消息里一样，用「奇数行向右缩进半格」的行列 (col,row)
static func hex_label(h: Vector2i) -> String:
	return "(%d,%d)" % [h.x + (h.y - (h.y & 1)) / 2, h.y]


static func hex_distance(a: Vector2i, b: Vector2i) -> int:
	var dq := a.x - b.x
	var dr := a.y - b.y
	return (absi(dq) + absi(dr) + absi(dq + dr)) / 2


# ---------------------------------------------------------------- 名称

## 配置里 `table` 表中 `key` 的中文名；表项是字符串或带 name 的表
func cname(table: String, key) -> String:
	var t = config.get(table, {})
	if not (t is Dictionary) or not t.has(str(key)):
		return str(key)
	var e = t[str(key)]
	if e is Dictionary:
		return str(e.get("name", key))
	return str(e)


func person_name(id) -> String:
	if id == null:
		return "无"
	var c = characters.get(int(id))
	return str(c.name) if c != null else "人物%d" % int(id)


func country_name(id) -> String:
	if id == null:
		return "无"
	var c = countries.get(int(id))
	return str(c.name) if c != null else "国%d" % int(id)


func city_name(id) -> String:
	if id == null:
		return "无"
	var c = my_cities.get(int(id), foreign_cities.get(int(id)))
	return str(c.name) if c != null else "城%d" % int(id)


func party_name(p: Dictionary) -> String:
	return country_name(p.id) if p.kind == "country" else person_name(p.id)


func mode_name(m) -> String:
	return {"guan": "官营", "min": "民营"}.get(str(m), str(m))


## 物资清单：「粮 19.9、青铜 40」，略去不到 0.05 的
func inv_text(inv) -> String:
	if inv == null or inv.is_empty():
		return "无"
	var parts := []
	for k in inv:
		if absi(int(inv[k])) < 50:
			continue
		parts.append("%s %s" % [cname("goods", k), fx(inv[k])])
	return "、".join(parts) if not parts.is_empty() else "无"


# ---------------------------------------------------------------- 定点数（实现在 num.gd）

const Num := preload("res://scripts/num.gd")


static func fx(x) -> String:
	return Num.fx(x)


static func pct(x) -> int:
	return Num.pct(x)


static func parse_fx(text: String) -> int:
	return Num.parse_fx(text)


static func fx_input(x) -> String:
	return Num.fx_input(x)


## 封君征讨封臣的理由
func grievances_text(list: Array) -> String:
	var parts := []
	for g in list:
		match str(g.kind):
			"low_reputation":
				parts.append("风评太低")
			"usurpation":
				parts.append("僭越")
			"tribute_shortfall":
				parts.append("连续 %d 年纳贡不足" % int(g.years))
			"defiance":
				parts.append("违抗国君")
			_:
				parts.append(str(g.kind))
	return "、".join(parts)




func task_text(t: Dictionary) -> String:
	match str(t.kind):
		"produce":
			return "%s生产%s" % [mode_name(t.mode), cname("recipes", t.recipe)]
		"construct":
			return "%s营建%s" % [mode_name(t.mode), cname("buildings", t.building)]
		"reclaim":
			return "开垦%s" % cname("terrains", t.terrain)
		"improve":
			return "修%s" % cname("improvements", t.improvement)
		"collect_tax":
			return "收税"
		"repair_wall":
			return "修城墙"
		"sacrifice":
			return "祭祀"
	return str(t.kind)


func unit_total(units: Dictionary) -> int:
	var n := 0
	for k in units:
		n += int(units[k])
	return n


func army_size(a: Dictionary) -> int:
	if a.has("squads"):
		var n := 0
		for s in a.squads:
			n += int(s.count)
		return n
	return unit_total(a.get("units", {}))


## 一个人物的代表色：地图上城的疆域、军队都用这个颜色
static func person_color(id) -> Color:
	if id == null:
		return Color(0.6, 0.6, 0.6)
	var h := fmod(int(id) * 0.618034 + 0.08, 1.0)
	return Color.from_hsv(h, 0.65, 0.9)


func city_owner(id: int):
	var c = my_cities.get(id, foreign_cities.get(id))
	return null if c == null else c.get("owner")


func city_occupier(id: int):
	var c = my_cities.get(id, foreign_cities.get(id))
	return null if c == null else c.get("occupier")


## 自己能统领其军队的城：所有者；被攻占的城是占领者
func commandable_cities() -> Array:
	var out := []
	for id in my_cities:
		var c: Dictionary = my_cities[id]
		if c.occupied:
			if c.occupier != null and int(c.occupier) == me:
				out.append(c)
		elif c.owner != null and int(c.owner) == me:
			out.append(c)
	return out
