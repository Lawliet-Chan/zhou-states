## 主界面：登录、顶栏、地图、右侧详情、底部消息。
## 固定布局来自场景，动态内容按服务器回的结果刷新。
extends Control

const TITLE := "其惟春秋"
const PANEL_WIDTH := 400

var map_view: Control
var login_box: Control
var game_box: Control
var url_edit: LineEdit
var player_edit: SpinBox
var login_status: Label
var date_label: Label
var me_label: Label
var waiting_label: Label
var end_turn_btn: Button
var hint_label: Label
var detail: VBoxContainer
var world: VBoxContainer
var log_box: RichTextLabel
var tabs: TabContainer

## 选中的东西：{kind: cell/city/army/foreign_army, id, hex}
var sel := {}
## 选目标：""、"attack"、"ranged"
var target_mode := ""
var force_march := false

var _logged_turn := -1
var _logged_count := 0
var _args := {}


func _ready() -> void:
	# 系统字体：macOS 的苹方是字体集，Godot 可能挑到繁体的那一款，缺简体字，所以冬青黑体（GB）放前面
	theme = _make_theme()
	_build_login()
	_build_game()
	game_box.visible = false
	Net.opened.connect(_on_opened)
	Net.closed.connect(_on_closed)
	Game.logged_in.connect(_on_logged_in)
	Game.view_changed.connect(_on_view_changed)
	Game.waiting_changed.connect(_on_waiting_changed)
	Game.turn_ended.connect(_on_turn_ended)
	Game.command_result.connect(_on_command_result)
	Game.server_error.connect(func(m): _log("[color=#ff8080]服务器：%s[/color]" % m))
	# 命令行（联调用）：-- --server=ws://... --player=3 自动登录；
	# --cmd='{"type":...}' 登录后下指令（可以写多个）；--select=q,r 选中一格；--press=按钮文字 按下详情里的按钮；
	# --tab=1 切到第几页；--screenshot=path 截图后退出
	_args["cmd"] = []
	for a in OS.get_cmdline_user_args():
		var kv: PackedStringArray = a.trim_prefix("--").split("=", true, 1)
		var val := kv[1] if kv.size() > 1 else ""
		if kv[0] == "cmd":
			_args.cmd.append(JSON.parse_string(val))
		else:
			_args[kv[0]] = val
	if _args.has("server"):
		url_edit.text = _args.server
	if _args.has("player"):
		player_edit.value = int(_args.player)
		_connect()


func _make_theme() -> Theme:
	var t := Theme.new()
	var f := SystemFont.new()
	f.font_names = PackedStringArray(["Hiragino Sans GB", "PingFang SC", "Heiti SC", "Noto Sans CJK SC", "Microsoft YaHei", "sans-serif"])
	t.default_font = f
	t.default_font_size = 15
	return t


# ---------------------------------------------------------------- 搭界面

func _build_login() -> void:
	login_box = CenterContainer.new()
	login_box.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(login_box)
	var panel := PanelContainer.new()
	login_box.add_child(panel)
	var v := _vbox(panel, 12)
	var m := MarginContainer.new()
	v.add_child(m)
	var title := Label.new()
	title.text = TITLE
	title.add_theme_font_size_override("font_size", 48)
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	v.add_child(title)
	var sub := Label.new()
	sub.text = "周代列国"
	sub.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	sub.modulate = Color(1, 1, 1, 0.6)
	v.add_child(sub)
	url_edit = LineEdit.new()
	url_edit.text = "ws://127.0.0.1:7878"
	url_edit.custom_minimum_size = Vector2(320, 0)
	_row(v, "服务器", url_edit)
	player_edit = SpinBox.new()
	player_edit.min_value = 0
	player_edit.max_value = 1 << 30
	player_edit.value = 3
	_row(v, "人物编号", player_edit)
	var btn := Button.new()
	btn.text = "入朝"
	btn.pressed.connect(_connect)
	v.add_child(btn)
	login_status = Label.new()
	login_status.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	v.add_child(login_status)


## 固定布局由场景提供，脚本只绑定交互和填充服务器数据。
func _build_game() -> void:
	game_box = $Game
	var top := $Game/TopBar/Row
	date_label = top.get_node("Date")
	me_label = top.get_node("Character")
	waiting_label = top.get_node("Waiting")
	end_turn_btn = top.get_node("EndTurn")
	end_turn_btn.toggled.connect(func(on): Game.end_turn(on))
	map_view = $Game/Middle/Map/View
	hint_label = $Game/Middle/Map/Hint
	map_view.hex_clicked.connect(_on_hex_clicked)
	map_view.hex_right_clicked.connect(_on_hex_right_clicked)
	tabs = $Game/Middle/RightPanel
	detail = tabs.get_node("详情/Margin/Content")
	world = tabs.get_node("天下/Margin/Content")
	log_box = $Game/LogPanel/Log


func _vbox(parent: Node, sep := 6) -> VBoxContainer:
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", sep)
	parent.add_child(v)
	return v


func _row(parent: Node, label: String, ctrl: Control) -> HBoxContainer:
	var h := HBoxContainer.new()
	var l := Label.new()
	l.text = label
	l.custom_minimum_size = Vector2(80, 0)
	h.add_child(l)
	ctrl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	h.add_child(ctrl)
	parent.add_child(h)
	return h


# ---- 详情面板里常用的几样

func _clear(box: Node) -> void:
	for c in box.get_children():
		box.remove_child(c)
		c.queue_free()


func _header(box: Node, text: String) -> void:
	var l := Label.new()
	l.text = text
	l.add_theme_font_size_override("font_size", 20)
	box.add_child(l)


func _section(box: Node, text: String) -> void:
	box.add_child(HSeparator.new())
	var l := Label.new()
	l.text = text
	l.add_theme_color_override("font_color", Color("#e8c66a"))
	box.add_child(l)


func _kv(box: Node, k: String, v) -> void:
	var l := Label.new()
	l.text = "%s：%s" % [k, str(v)]
	l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(l)


func _note(box: Node, text: String) -> void:
	var l := Label.new()
	l.text = text
	l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	l.modulate = Color(1, 1, 1, 0.65)
	box.add_child(l)


func _buttons(box: Node) -> HFlowContainer:
	var f := HFlowContainer.new()
	box.add_child(f)
	return f


func _btn(parent: Node, text: String, cb: Callable, disabled := false) -> Button:
	var b := Button.new()
	b.text = text
	b.disabled = disabled
	b.pressed.connect(cb)
	parent.add_child(b)
	return b


# ---------------------------------------------------------------- 连接

func _connect() -> void:
	var err := Net.open(url_edit.text.strip_edges())
	login_status.text = "正在连接……" if err == OK else "连不上：%s" % error_string(err)


func _on_opened() -> void:
	login_status.text = "已连接，正在登录……"
	Game.login(int(player_edit.value))


func _on_closed() -> void:
	login_status.text = "连接断开"
	if game_box.visible:
		_log("[color=#ff8080]与服务器的连接断开了[/color]")
		login_box.visible = true
		game_box.visible = false


func _on_logged_in() -> void:
	login_box.visible = false
	game_box.visible = true
	_log("[b]%s[/b]：以%s的身份登录" % [TITLE, Game.person_name(Game.me)])
	if Game.last_turn != null:
		_log_events(Game.last_turn, "上一回合")
	for cmd in _args.cmd:
		if cmd is Dictionary:
			Game.send_command(cmd)
	if _args.has("screenshot"):
		_screenshot(_args.screenshot)


func _screenshot(path: String) -> void:
	await get_tree().create_timer(1.5).timeout
	if _args.has("select"):
		var p: PackedStringArray = _args.select.split(",")
		_on_hex_clicked(Vector2i(int(p[0]), int(p[1])))
		await get_tree().create_timer(0.5).timeout
	if _args.has("press"):
		var b := _find_button(detail, _args.press)
		if b != null:
			b.pressed.emit()
		await get_tree().create_timer(0.5).timeout
	if _args.has("tab"):
		tabs.current_tab = int(_args.tab)
		await get_tree().create_timer(0.3).timeout
	await RenderingServer.frame_post_draw
	get_viewport().get_texture().get_image().save_png(path)
	get_tree().quit()


## 按文字找按钮（联调时 --press 用）
func _find_button(node: Node, text: String) -> Button:
	for c in node.get_children():
		if c is Button and c.text == text:
			return c
		var found := _find_button(c, text)
		if found != null:
			return found
	return null


# ---------------------------------------------------------------- 服务器的消息

func _on_view_changed() -> void:
	var v := Game.view
	date_label.text = "第 %d 回合　%d 年 %d 月（%s）" % [int(v.turn), int(v.year), int(v.month), str(v.season)]
	var ch = v.character
	if ch != null:
		me_label.text = "%s（%s）　风评 %s　私库：%s" % [ch.name, Game.cname("ranks", ch.rank), Game.fx(ch.reputation), Game.inv_text(ch.private_store)]
	_log_events(v, "")
	_validate_selection()
	_refresh_detail()
	_refresh_world()
	map_view.queue_redraw()


func _on_waiting_changed() -> void:
	end_turn_btn.set_pressed_no_signal(Game.ready_ids.has(Game.me))
	end_turn_btn.text = "已结束回合（点击取消）" if end_turn_btn.button_pressed else "结束回合"
	if Game.waiting_ids.is_empty():
		waiting_label.text = ""
	else:
		waiting_label.text = "还在等：" + "、".join(Game.waiting_ids.map(func(id): return Game.person_name(id)))


func _on_turn_ended(report: Dictionary) -> void:
	_log("[color=#e8c66a]—— 第 %d 回合结算完毕 ——[/color]" % int(report.turn))
	# 结算后的报告里就是新的情况；再要一份，免得漏了回合开始时的变化
	Game.request_view()


func _on_command_result(_req: int, ok: bool, ids: Array, error: String, desc: String) -> void:
	if ok:
		var extra := "" if ids.is_empty() else "（新编号 %s）" % ", ".join(ids.map(func(x): return str(int(x))))
		_log("[color=#9fd89f]✓ %s%s[/color]" % [desc, extra])
	else:
		_log("[color=#ff8080]✗ %s：%s[/color]" % [desc, error])


func _log(text: String) -> void:
	log_box.append_text(text + "\n")


## 把报告里还没记过的消息记到日志里
func _log_events(report: Dictionary, label: String) -> void:
	var t := int(report.turn)
	var events: Array = report.events
	var start := 0
	if label != "":
		_log("[color=#e8c66a]—— %s（第 %d 回合）——[/color]" % [label, t])
	elif t == _logged_turn:
		start = _logged_count
	for i in range(start, events.size()):
		var e: Dictionary = events[i]
		var mark := "【公告】" if e.get("public", false) else ""
		_log(mark + str(e.text))
	if label == "":
		_logged_turn = t
		_logged_count = events.size()


# ---------------------------------------------------------------- 地图上的操作

func _on_hex_clicked(h: Vector2i) -> void:
	if not Game.cells.has(h):
		return
	if target_mode != "":
		var army := int(sel.get("id", -1))
		var ranged := target_mode == "ranged"
		_set_target_mode("")
		Game.send_command({"type": "attack", "army": army, "at": Game.hd(h), "ranged": ranged},
			"军队 %d %s %s" % [army, "远程攻击" if ranged else "攻击", _hex_text(h)])
		return
	var armies: Array = map_view.armies_on(h)
	# 同一格连点：在军队和城、格子之间轮换
	var choices := []
	for a in armies:
		choices.append({"kind": "army" if a.own else "foreign_army", "id": int(a.data.id), "hex": h})
	if Game.tile_city.has(h):
		choices.append({"kind": "city", "id": int(Game.tile_city[h]), "hex": h})
	choices.append({"kind": "cell", "id": -1, "hex": h})
	var idx := 0
	for i in choices.size():
		if choices[i].hex == sel.get("hex") and choices[i].kind == sel.get("kind") and choices[i].id == sel.get("id"):
			idx = (i + 1) % choices.size()
	_select(choices[idx])


func _on_hex_right_clicked(h: Vector2i) -> void:
	if target_mode != "":
		_set_target_mode("")
		return
	if sel.get("kind") != "army" or not Game.cells.has(h):
		return
	var army := int(sel.id)
	Game.send_command({"type": "march_army", "army": army, "to": Game.hd(h), "force": force_march},
		"军队 %d 行军到 %s%s" % [army, _hex_text(h), "（硬闯）" if force_march else ""])


func _select(s: Dictionary) -> void:
	sel = s
	map_view.selected_hex = s.get("hex")
	map_view.selected_army = int(s.id) if s.kind == "army" else -1
	_set_target_mode("")
	_refresh_detail()


func _validate_selection() -> void:
	if sel.is_empty():
		return
	match sel.kind:
		"army":
			var a = Game.my_armies.get(int(sel.id))
			if a == null:
				sel = {"kind": "cell", "id": -1, "hex": sel.hex}
				map_view.selected_army = -1
			else:
				# 军队走了，选中框跟着走
				sel.hex = Game.hx(a.pos)
				map_view.selected_hex = sel.hex
		"foreign_army":
			var a = Game.foreign_armies.get(int(sel.id))
			if a == null:
				sel = {"kind": "cell", "id": -1, "hex": sel.hex}


func _set_target_mode(mode: String) -> void:
	target_mode = mode
	map_view.highlight.clear()
	if mode != "" and sel.get("kind") == "army":
		var a = Game.my_armies.get(int(sel.id))
		if a != null:
			var reach := 2 if mode == "ranged" else 1
			for h in Game.cells:
				var d := Game.hex_distance(h, Game.hx(a.pos))
				if d >= 1 and d <= reach:
					map_view.highlight[h] = Color("#ff5a4a")
		hint_label.text = "点一格作为攻击目标，右键取消"
	else:
		hint_label.text = "左键选中（同一格连点轮换），右键让选中的军队行军，拖动平移，滚轮缩放"
	map_view.queue_redraw()


func _hex_text(h: Vector2i) -> String:
	var c = Game.cells.get(h)
	var name := Game.cname("terrains", c.terrain) if c != null else "?"
	if Game.tile_city.has(h):
		name = Game.city_name(Game.tile_city[h]) + "·" + name
	return name + Game.hex_label(h)


# ---------------------------------------------------------------- 详情面板

func _refresh_detail() -> void:
	_clear(detail)
	if sel.is_empty():
		_note(detail, "在地图上点一格看详情。")
		_my_overview()
		return
	match sel.kind:
		"army":
			_army_detail(Game.my_armies.get(int(sel.id)))
		"foreign_army":
			_foreign_army_detail(Game.foreign_armies.get(int(sel.id)))
		"city":
			var id := int(sel.id)
			if Game.my_cities.has(id):
				_city_detail(Game.my_cities[id])
			elif Game.foreign_cities.has(id):
				_foreign_city_detail(Game.foreign_cities[id])
			else:
				_cell_detail(sel.hex)
		_:
			_cell_detail(sel.hex)


func _my_overview() -> void:
	_section(detail, "我的城")
	for id in Game.my_cities:
		var c: Dictionary = Game.my_cities[id]
		_btn(detail, "%s　人口 %d" % [c.name, int(c.population)], _select.bind({"kind": "city", "id": int(id), "hex": Game.hx(c.seat)}))
	_section(detail, "我的军队")
	if Game.my_armies.is_empty():
		_note(detail, "没有军队。在自己的城里可以征发。")
	for id in Game.my_armies:
		var a: Dictionary = Game.my_armies[id]
		_btn(detail, "军队 %d　%d 人　在 %s" % [int(id), Game.army_size(a), _hex_text(Game.hx(a.pos))], _select.bind({"kind": "army", "id": int(id), "hex": Game.hx(a.pos)}))


func _cell_detail(h: Vector2i) -> void:
	var c = Game.cells.get(h)
	if c == null:
		return
	var t: Dictionary = Game.config.terrains.get(str(c.terrain), {})
	_header(detail, "%s　%s" % [Game.cname("terrains", c.terrain), Game.hex_label(h)])
	if Game.tile_city.has(h):
		_kv(detail, "属于", Game.city_name(Game.tile_city[h]))
	else:
		_kv(detail, "属于", "野地")
	if t.has("move_cost"):
		_kv(detail, "行军耗费", t.move_cost)
	else:
		_kv(detail, "行军", "过不去")
	if t.has("defense_bonus"):
		_kv(detail, "守军防御", "+%d%%" % Game.pct(t.defense_bonus))
	_kv(detail, "视野", "看得到" if Game.vision.has(h) else "看不到")
	_back_button()


func _back_button() -> void:
	detail.add_child(HSeparator.new())
	_btn(detail, "返回总览", func():
		sel = {}
		map_view.selected_hex = null
		map_view.selected_army = -1
		_set_target_mode("")
		_refresh_detail())


func _city_detail(c: Dictionary) -> void:
	var id := int(c.id)
	_header(detail, "%s" % c.name)
	_kv(detail, "所有者", Game.person_name(c.owner))
	_kv(detail, "管理者", Game.person_name(c.manager))
	_kv(detail, "国家", Game.country_name(c.country))
	if c.liege != null:
		_kv(detail, "封君", Game.person_name(c.liege))
	if c.occupied:
		_kv(detail, "被攻占", "占领者 %s" % Game.person_name(c.occupier))
	_kv(detail, "人口", "%d / %d" % [int(c.population), int(c.housing)])
	_kv(detail, "城防", "城墙 %s　城中心 %s" % ["无" if c.wall == null else Game.fx(c.wall), Game.fx(c.center)])
	_kv(detail, "士人", "%d（闲 %d）" % [c.officials.size(), int(c.idle_officials)])
	if int(c.neighbor_bonus) > 0:
		_kv(detail, "邻城增益", "+%d%%" % Game.pct(c.neighbor_bonus))

	var acts := _buttons(detail)
	var here = Game.view.character.location if Game.view.character != null else null
	_btn(acts, "搬到这里", func(): Game.send_command({"type": "move_to", "city": id}, "搬到%s" % c.name), here != null and int(here) == id)
	var can_raise := Game.commandable_cities().any(func(x): return int(x.id) == id)
	_btn(acts, "征发军队", _raise_army_dialog.bind(c), not can_raise)

	_city_management(c)
	_city_fief(c)

	_section(detail, "府库")
	_note(detail, Game.inv_text(c.treasury))
	_section(detail, "民间财富")
	_note(detail, Game.inv_text(c.private_wealth))
	_section(detail, "今年产出")
	_note(detail, Game.inv_text(c.income))
	if not c.buildings.is_empty():
		_section(detail, "建筑")
		var parts := []
		for k in c.buildings:
			parts.append("%s ×%d" % [Game.cname("buildings", k), int(c.buildings[k])])
		_note(detail, "、".join(parts))

	_section(detail, "人群")
	for g in c.groups:
		var skills: Array = g.skills.map(func(s): return Game.cname("skills", s))
		_kv(detail, "%s %d 人" % [g.name, int(g.size)], "、".join(skills))
		_note(detail, "　愉悦 %s　教化 %s　忠诚 %s%s" % [Game.fx(g.happiness), Game.fx(g.culture), Game.fx(g.loyalty),
			"　敌视 %s" % Game.fx(g.hostility) if int(g.hostility) > 0 else ""])

	_section(detail, "工事")
	if c.assignments.is_empty():
		_note(detail, "没有工事")
	for a in c.assignments:
		var t := Game.task_text(a.task)
		var row := HBoxContainer.new()
		var l := Label.new()
		l.text = "%s：%s，%d 人%s" % [a.shi, t, int(a.workers), "，进度 %s" % Game.fx(a.progress) if int(a.progress) > 0 else ""]
		l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		row.add_child(l)
		var aid := int(a.id)
		_btn(row, "撤", func(): Game.send_command({"type": "cancel_assignment", "city": id, "assignment": aid}, "%s撤掉「%s」" % [c.name, t]))
		detail.add_child(row)
	if not c.learning.is_empty():
		_section(detail, "学技能")
		for l in c.learning:
			_note(detail, str(l))
	_back_button()


# ---- 管理：任命管理者、收回管理权、规定任务、上缴

func _city_management(c: Dictionary) -> void:
	var id := int(c.id)
	var owner_is_me: bool = c.owner != null and int(c.owner) == Game.me
	var manager_is_me: bool = c.manager != null and int(c.manager) == Game.me
	var managed_by_other: bool = c.manager != null and int(c.manager) != int(c.owner)
	if not owner_is_me and not managed_by_other:
		return
	_section(detail, "管理")
	if owner_is_me:
		if c.manager_offer != null:
			_note(detail, "已任命%s为管理者，等对方答复" % Game.person_name(c.manager_offer))
		var b := _buttons(detail)
		_btn(b, "任命管理者", _person_dialog.bind("任命%s的管理者" % c.name,
			"对方接受后才生效。管理者打理城里的一切，你可以给他定每年上缴的规定任务。",
			func(to): Game.send_command({"type": "offer_management", "city": id, "to": to}, "任命%s管理%s" % [Game.person_name(to), c.name])))
		if managed_by_other:
			_btn(b, "收回管理权", func(): _confirm("收回%s的管理权，自己管理？" % c.name,
				func(): Game.send_command({"type": "revoke_management", "city": id}, "收回%s的管理权" % c.name)))

	# 规定任务：所有者交给管理者的每年上缴
	if managed_by_other:
		var q = c.quota
		if q == null:
			_note(detail, "规定任务：没有")
		else:
			_kv(detail, "规定任务（每年）", Game.inv_text(q.goods))
			_kv(detail, "今年已上缴", Game.inv_text(q.delivered))
		var b := _buttons(detail)
		if owner_is_me:
			_btn(b, "设定规定任务", _goods_dialog.bind("给%s的管理者定规定任务" % c.name,
				"每年要上缴到你私库的物资。年末结算，完成得好坏影响你对管理者的信任。",
				{} if q == null else q.goods,
				func(goods): Game.send_command({"type": "set_quota", "city": id, "goods": goods}, "给%s定规定任务：%s" % [c.name, Game.inv_text(goods)])))
			if q != null:
				_btn(b, "取消规定任务", func(): Game.send_command({"type": "set_quota", "city": id, "goods": {}}, "取消%s的规定任务" % c.name))
		if manager_is_me:
			_btn(b, "上缴", _goods_dialog.bind("从%s府库上缴" % c.name,
				"上缴到所有者%s的私库，计入今年的规定任务。府库：%s" % [Game.person_name(c.owner), Game.inv_text(c.treasury)],
				{} if q == null else _remaining(q.goods, q.delivered),
				func(goods): Game.send_command({"type": "deliver", "city": id, "goods": goods}, "%s上缴 %s" % [c.name, Game.inv_text(goods)])))


# ---- 封授、纳贡（自己的城这一边）

func _city_fief(c: Dictionary) -> void:
	var id := int(c.id)
	var owner_is_me: bool = c.owner != null and int(c.owner) == Game.me
	if not owner_is_me and c.liege == null:
		return
	_section(detail, "封授与纳贡")
	# 封地不能再往下封
	if owner_is_me and c.liege == null:
		if c.fief_offer != null:
			_note(detail, "已封授给%s，等对方答复" % Game.person_name(c.fief_offer))
		var b := _buttons(detail)
		_btn(b, "封授给……", _person_dialog.bind("把%s封授给别人" % c.name,
			"对方接受后成为这座城的所有者，你成为他的封君，他每年向你纳贡。封出去的城不能收回。",
			func(to): _confirm("把%s封授给%s？封出去的城不能收回。" % [c.name, Game.person_name(to)],
				func(): Game.send_command({"type": "offer_fief", "city": id, "to": to}, "把%s封授给%s" % [c.name, Game.person_name(to)]))))
	if c.liege == null:
		return
	_tribute_info(detail, c.tribute, bool(c.tribute_excessive))
	if not c.grievances.is_empty():
		_kv(detail, "封君征讨你的理由", Game.grievances_text(c.grievances))
	if owner_is_me:
		var b := _buttons(detail)
		var t = c.tribute
		_btn(b, "纳贡", _goods_dialog.bind("%s向封君纳贡" % c.name,
			"从府库送到封君%s的私库，计入今年的纳贡。府库：%s" % [Game.person_name(c.liege), Game.inv_text(c.treasury)],
			{} if t == null else _remaining(t.goods, t.delivered),
			func(goods): Game.send_command({"type": "pay_tribute", "city": id, "goods": goods}, "%s纳贡 %s" % [c.name, Game.inv_text(goods)])))
		if c.tribute_excessive and t != null and not t.refused:
			_btn(b, "拒绝纳贡", func(): _confirm("封君要的纳贡过火，拒绝？",
				func(): Game.send_command({"type": "refuse_tribute", "city": id}, "%s拒绝过火的纳贡" % c.name)))


## 纳贡的情况：指定的还是按比例自动的、交了多少、过不过火
func _tribute_info(box: Node, t, excessive: bool) -> void:
	if t == null:
		_kv(box, "纳贡", "按比例自动纳贡：每年府库产出的 %d%%" % Game.pct(Game.config.fief.auto_rate))
		return
	_kv(box, "指定纳贡（每年）", Game.inv_text(t.goods))
	_kv(box, "今年已纳", Game.inv_text(t.delivered))
	if t.refused:
		_note(box, "已拒绝这份纳贡")
	elif excessive:
		_note(box, "这份纳贡过火了，封臣可以拒绝，封地百姓会敌视封君")


## 还差多少：要的减去已交的
func _remaining(want: Dictionary, done: Dictionary) -> Dictionary:
	var out := {}
	for k in want:
		var left := int(want[k]) - int(done.get(k, 0))
		if left > 0:
			out[k] = left
	return out


# ---- 封臣的城（自己是封君这一边）

func _vassal_city(c: Dictionary) -> void:
	var id := int(c.id)
	var f: Dictionary = c.fief
	_section(detail, "封臣")
	_tribute_info(detail, f.tribute, bool(f.tribute_excessive))
	if not f.grievances.is_empty():
		_kv(detail, "征讨的理由", Game.grievances_text(f.grievances))
	var b := _buttons(detail)
	_btn(b, "指定纳贡", _goods_dialog.bind("给%s指定每年的纳贡" % c.name,
		"取代按比例自动纳贡。要得太多（超过它一年产出的 %d%%）就是过火：封臣可以拒绝，百姓会敌视你。" % Game.pct(Game.config.fief.excessive_ratio),
		{} if f.tribute == null else f.tribute.goods,
		func(goods): Game.send_command({"type": "set_tribute", "city": id, "goods": goods}, "给%s指定纳贡：%s" % [c.name, Game.inv_text(goods)])))
	if f.tribute != null:
		_btn(b, "恢复自动纳贡", func(): Game.send_command({"type": "set_tribute", "city": id, "goods": {}}, "%s恢复按比例自动纳贡" % c.name))


func _foreign_city_detail(c: Dictionary) -> void:
	_header(detail, "%s（%s）" % [c.name, c.size])
	_kv(detail, "所有者", Game.person_name(c.owner))
	_kv(detail, "国家", Game.country_name(c.country))
	if c.occupier != null:
		_kv(detail, "被攻占", "占领者 %s" % Game.person_name(c.occupier))
	_kv(detail, "城墙", "已破" if c.wall_broken else ("有" if c.wall else "无"))
	_kv(detail, "消息", "眼下看得到" if c.visible else "第 %d 回合所见" % int(c.turn))
	if c.fief != null:
		_vassal_city(c)
	if c.owner != null and int(c.owner) != Game.me:
		var acts := _buttons(detail)
		var owner := int(c.owner)
		_btn(acts, "向%s宣战" % Game.person_name(owner), _declare_war.bind({"kind": "person", "id": owner}))
	_back_button()


func _army_detail(a) -> void:
	if a == null:
		return
	var id := int(a.id)
	_header(detail, "军队 %d　%d 人" % [id, Game.army_size(a)])
	_kv(detail, "编成于", Game.city_name(a.city))
	_kv(detail, "位置", _hex_text(Game.hx(a.pos)))
	_kv(detail, "目的地", "无" if a.dest == null else _hex_text(Game.hx(a.dest)))
	_kv(detail, "行军力", int(a.moves_left))
	_kv(detail, "出征", "是（要靠粮道供粮）" if a.on_campaign else "否（在自己的城的地上）")
	if int(a.hunger) > 0:
		_kv(detail, "断粮", "%d 回合，战斗力剩 %d%%" % [int(a.hunger), Game.pct(a.strength)])
	if a.supply != null:
		var s: Dictionary = a.supply
		_kv(detail, "粮道", "从%s，辎重 %d 人%s" % [Game.city_name(s.from), int(s.convoy), "，找不到路！" if s.path.is_empty() else ""])

	_section(detail, "兵")
	for s in a.squads:
		var extra := "" if s.gear.is_empty() else "　带 " + Game.inv_text(s.gear)
		_note(detail, "%s%s %d 人　装备 %d%%%s" % ["精锐" if s.elite else "", Game.cname("units", s.unit), int(s.count), Game.pct(s.equipped), extra])

	_section(detail, "号令")
	_note(detail, "右键地图上的一格行军；走不到的回合会接着走。")
	var fm := CheckBox.new()
	fm.text = "硬闯（穿过没有通行权的城，等于宣战）"
	fm.button_pressed = force_march
	fm.toggled.connect(func(on): force_march = on)
	detail.add_child(fm)
	var acts := _buttons(detail)
	_btn(acts, "攻击", _set_target_mode.bind("attack"))
	_btn(acts, "远程攻击", _set_target_mode.bind("ranged"))
	if a.dest != null:
		_btn(acts, "停止行军", func(): Game.send_command({"type": "march_army", "army": id, "to": a.pos}, "军队 %d 停止行军" % id))
	_btn(acts, "上城墙", func(): Game.send_command({"type": "man_wall", "army": id, "on": true}, "军队 %d 上城墙" % id))
	_btn(acts, "下城墙", func(): Game.send_command({"type": "man_wall", "army": id, "on": false}, "军队 %d 下城墙" % id))
	if a.supply == null:
		_btn(acts, "开粮道", _supply_dialog.bind(a))
	else:
		_btn(acts, "撤粮道", func(): Game.send_command({"type": "close_supply_line", "army": id}, "军队 %d 撤粮道" % id))
	_btn(acts, "遣散", func(): Game.send_command({"type": "disband_army", "army": id}, "遣散军队 %d" % id))

	# 在被攻占的城里：劫掠、接管
	var h := Game.hx(a.pos)
	if Game.tile_city.has(h):
		var cid := int(Game.tile_city[h])
		var occ = Game.city_occupier(cid)
		if occ != null and int(occ) == Game.me:
			_section(detail, "占领%s" % Game.city_name(cid))
			var pa := _buttons(detail)
			_btn(pa, "劫掠府库", func(): Game.send_command({"type": "plunder", "city": cid, "target": "treasury"}, "劫掠%s府库" % Game.city_name(cid)))
			_btn(pa, "劫掠民间", func(): Game.send_command({"type": "plunder", "city": cid, "target": "citizens"}, "劫掠%s民间" % Game.city_name(cid)))
	_back_button()


func _foreign_army_detail(a) -> void:
	if a == null:
		return
	_header(detail, "%s的军队　%d 人" % [Game.person_name(a.commander), Game.army_size(a)])
	_kv(detail, "位置", _hex_text(Game.hx(a.pos)))
	_kv(detail, "消息", "眼下看得到" if a.visible else "第 %d 回合所见，之后去向不明" % int(a.turn))
	for u in a.units:
		_note(detail, "%s %d 人" % [Game.cname("units", u), int(a.units[u])])
	_back_button()


# ---------------------------------------------------------------- 天下：人物、国家、战争

func _refresh_world() -> void:
	_clear(world)
	var offers: Array = Game.view.get("offers", [])
	if not offers.is_empty():
		_header(world, "待你答复")
		for o in offers:
			var cid := int(o.city)
			var fief: bool = o.kind == "fief"
			_kv(world, "封授" if fief else "管理任命", "%s要把%s%s" % [Game.person_name(o.from), o.city_name, "封给你" if fief else "交给你管理"])
			if fief:
				_note(world, "接受后你成为这座城的所有者，%s成为你的封君，你每年向他纳贡。" % Game.person_name(o.from))
			var b := _buttons(world)
			if fief:
				_btn(b, "受封", func(): Game.send_command({"type": "accept_fief", "city": cid}, "受封%s" % o.city_name))
				_btn(b, "辞封", func(): Game.send_command({"type": "decline_fief", "city": cid}, "辞封%s" % o.city_name))
			else:
				_btn(b, "接受任命", func(): Game.send_command({"type": "accept_management", "city": cid}, "接受管理%s" % o.city_name))
				_btn(b, "拒绝任命", func(): Game.send_command({"type": "decline_management", "city": cid}, "拒绝管理%s" % o.city_name))
		world.add_child(HSeparator.new())
	_header(world, "战争")
	if Game.view.wars.is_empty():
		_note(world, "天下太平。")
	for w in Game.view.wars:
		var wid := int(w.id)
		_kv(world, "战争 %d" % wid, "%s 攻 %s%s%s" % [Game.party_name(w.attacker), Game.party_name(w.defender),
			"" if w.justified else "（无故开战）", "（已被制止）" if w.get("halted", false) else ""])
		if w.get("peace") != null:
			var p: Dictionary = w.peace
			_note(world, "%s提出讲和" % Game.person_name(p.from))
			if int(p.from) != Game.me:
				var b := _buttons(world)
				_btn(b, "接受讲和", func(): Game.send_command({"type": "accept_peace", "war": wid}, "接受战争 %d 的讲和" % wid))
				_btn(b, "拒绝讲和", func(): Game.send_command({"type": "decline_peace", "war": wid}, "拒绝战争 %d 的讲和" % wid))
		else:
			var b := _buttons(world)
			_btn(b, "求和（不附条件）", func(): Game.send_command({"type": "propose_peace", "war": wid}, "就战争 %d 提出讲和" % wid))

	_section(world, "列国")
	for id in Game.countries:
		var c: Dictionary = Game.countries[id]
		var row := HBoxContainer.new()
		var l := Label.new()
		l.text = "%s　国君 %s　都 %s" % [c.name, Game.person_name(c.ruler), Game.city_name(c.capital)]
		l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.add_child(l)
		var my_country = Game.view.character.nationality if Game.view.character != null else null
		if my_country == null or int(my_country) != int(id):
			_btn(row, "宣战", _declare_war.bind({"kind": "country", "id": int(id)}))
		world.add_child(row)

	_section(world, "人物")
	for id in Game.characters:
		var c: Dictionary = Game.characters[id]
		var row := HBoxContainer.new()
		var l := Label.new()
		l.text = "%s（%s）　%s　风评 %s" % [c.name, Game.cname("ranks", c.rank), Game.country_name(c.nationality), Game.fx(c.reputation)]
		l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		l.add_theme_color_override("font_color", Game.person_color(id).lightened(0.3))
		row.add_child(l)
		if int(id) != Game.me:
			_btn(row, "宣战", _declare_war.bind({"kind": "person", "id": int(id)}))
		world.add_child(row)


func _declare_war(target: Dictionary) -> void:
	var name := Game.party_name(target)
	_confirm("对%s宣战？" % name, func():
		Game.send_command({"type": "declare_war", "target": target}, "对%s宣战" % name))


# ---------------------------------------------------------------- 对话框

func _confirm(text: String, cb: Callable) -> void:
	var d: ConfirmationDialog = preload("res://scenes/confirm_dialog.tscn").instantiate()
	d.dialog_text = text
	d.ok_button_text = "是"
	d.cancel_button_text = "否"
	d.confirmed.connect(cb)
	d.visibility_changed.connect(func(): if not d.visible: d.queue_free())
	add_child(d)
	_popup(d)


## 弹出对话框：自动换行的文字会先把高度撑大，等排好版再收缩到内容的高度
func _popup(d: Window) -> void:
	d.popup_centered(Vector2i(520, 0))
	await get_tree().process_frame
	d.size = Vector2i(520, 0)
	d.move_to_center()


func _dialog(title: String) -> Array:
	var d: ConfirmationDialog = preload("res://scenes/dialog.tscn").instantiate()
	d.title = title
	d.ok_button_text = "确定"
	d.cancel_button_text = "取消"
	d.visibility_changed.connect(func(): if not d.visible: d.queue_free())
	var v: VBoxContainer = d.get_node("Content")
	add_child(d)
	return [d, v]


## 选一个人物（不含自己）
func _person_dialog(title: String, note: String, cb: Callable) -> void:
	var dv := _dialog(title)
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	_note(v, note)
	var ids := []
	var opt := OptionButton.new()
	for id in Game.characters:
		if int(id) == Game.me:
			continue
		var c: Dictionary = Game.characters[id]
		ids.append(int(id))
		opt.add_item("%s（%s，%s）" % [c.name, Game.cname("ranks", c.rank), Game.country_name(c.nationality)])
	_row(v, "人物", opt)
	d.get_ok_button().disabled = ids.is_empty()
	d.confirmed.connect(func(): cb.call(ids[opt.selected]))
	_popup(d)


## 填一份物资清单。数量写小数（1.5），发出去是千分之一为单位的整数
func _goods_dialog(title: String, note: String, initial: Dictionary, cb: Callable) -> void:
	var dv := _dialog(title)
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	_note(v, note)
	var keys: Array = Game.config.goods.keys()
	var list := VBoxContainer.new()
	v.add_child(list)
	var rows := []
	var add_row := func(good: String, amount: String):
		var r := HBoxContainer.new()
		var g := OptionButton.new()
		for k in keys:
			g.add_item(Game.cname("goods", k))
		g.selected = maxi(keys.find(good), 0)
		r.add_child(g)
		var n := LineEdit.new()
		n.text = amount
		n.placeholder_text = "数量"
		n.custom_minimum_size = Vector2(100, 0)
		r.add_child(n)
		var row := {"good": g, "amount": n, "node": r}
		_btn(r, "删", func():
			rows.erase(row)
			r.queue_free())
		rows.append(row)
		list.add_child(r)
	for k in initial:
		add_row.call(str(k), Game.fx_input(initial[k]))
	if initial.is_empty():
		add_row.call(str(keys[0]), "")
	_btn(v, "再加一样", func(): add_row.call(str(keys[0]), ""))
	d.confirmed.connect(func():
		var goods := {}
		for r in rows:
			var raw := Game.parse_fx(r.amount.text)
			if raw < 0:
				_log("[color=#ff8080]数量「%s」看不懂，要写数字，比如 1.5[/color]" % r.amount.text)
				return
			if raw == 0:
				continue
			var k: String = keys[r.good.selected]
			goods[k] = int(goods.get(k, 0)) + raw
		cb.call(goods))
	_popup(d)


## 会「军武」的人群
func _martial_groups(c: Dictionary) -> Array:
	var skill := str(Game.config.military.skill)
	return c.groups.filter(func(g): return g.skills.has(skill))


func _raise_army_dialog(c: Dictionary) -> void:
	var groups := _martial_groups(c)
	var dv := _dialog("在%s征发军队" % c.name)
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	if groups.is_empty():
		_note(v, "城里没有会「%s」的人。" % Game.cname("skills", Game.config.military.skill))
		d.get_ok_button().disabled = true
		_popup(d)
		return
	_note(v, "府库：" + Game.inv_text(c.treasury))
	var rows := []
	var list := VBoxContainer.new()
	v.add_child(list)
	var add_row := func():
		var r := HBoxContainer.new()
		var g := OptionButton.new()
		for x in groups:
			g.add_item("%s（%d 人）" % [x.name, int(x.size)])
		r.add_child(g)
		var u := OptionButton.new()
		var unit_keys := []
		for k in Game.config.units:
			unit_keys.append(k)
			u.add_item(Game.cname("units", k))
		r.add_child(u)
		var n := SpinBox.new()
		n.min_value = 1
		n.max_value = 100000
		n.value = 10
		n.suffix = "人"
		r.add_child(n)
		# 可选装备
		var extras := HFlowContainer.new()
		var refresh_extras := func():
			_clear(extras)
			var def: Dictionary = Game.config.units[unit_keys[u.selected]]
			for e in def.get("optional", {}):
				var cb := CheckBox.new()
				cb.text = Game.cname("goods", e)
				cb.set_meta("good", e)
				extras.add_child(cb)
		u.item_selected.connect(func(_i): refresh_extras.call())
		refresh_extras.call()
		list.add_child(r)
		list.add_child(extras)
		rows.append({"group": g, "unit": u, "count": n, "extras": extras, "keys": unit_keys})
	add_row.call()
	_btn(v, "再加一队", add_row)
	var cid := int(c.id)
	d.confirmed.connect(func():
		var squads := []
		for r in rows:
			var ex := []
			for cb in r.extras.get_children():
				if cb.button_pressed:
					ex.append(str(cb.get_meta("good")))
			squads.append({"group": int(groups[r.group.selected].id), "unit": str(r.keys[r.unit.selected]), "count": int(r.count.value), "extras": ex})
		Game.send_command({"type": "raise_army", "city": cid, "squads": squads}, "在%s征发军队" % c.name))
	_popup(d)


func _supply_dialog(a: Dictionary) -> void:
	var dv := _dialog("给军队 %d 开粮道" % int(a.id))
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	_note(v, "从起点城会「%s」的人里抽出辎重兵。打下来的城也可以当起点。" % Game.cname("skills", Game.config.military.skill))
	var cities := Game.my_cities.values()
	var city_opt := OptionButton.new()
	for c in cities:
		city_opt.add_item(str(c.name))
	_row(v, "起点城", city_opt)
	var group_opt := OptionButton.new()
	_row(v, "人群", group_opt)
	var groups := []
	var refresh := func():
		group_opt.clear()
		groups.clear()
		if cities.is_empty():
			return
		for g in _martial_groups(cities[city_opt.selected]):
			groups.append(g)
			group_opt.add_item("%s（%d 人）" % [g.name, int(g.size)])
	city_opt.item_selected.connect(func(_i): refresh.call())
	refresh.call()
	var aid := int(a.id)
	d.confirmed.connect(func():
		if groups.is_empty():
			_log("[color=#ff8080]这座城里没有会军武的人[/color]")
			return
		var c: Dictionary = cities[city_opt.selected]
		Game.send_command({"type": "open_supply_line", "army": aid, "from": int(c.id), "group": int(groups[group_opt.selected].id)},
			"从%s给军队 %d 开粮道" % [c.name, aid]))
	_popup(d)
