## 内政界面：仅组织输入，规则校验和结算交给服务器。
extends RefCounted

var ui
var _expanded := {}

func _init(main) -> void:
	ui = main

func can_manage(c: Dictionary) -> bool:
	return c.manager != null and int(c.manager) == Game.me

func send(c: Dictionary, kind: String, fields: Dictionary, label: String) -> void:
	var cmd := fields.duplicate(true)
	cmd.merge({"type": kind, "city": int(c.id)})
	Game.send_command(cmd, "%s：%s" % [c.name, label])

func city_actions(c: Dictionary, box: Node) -> void:
	ui._section(box, "内政")
	ui._kv(box, "税率", "%s%%" % Game.fx(int(c.tax_rate) * 100))
	if not can_manage(c):
		ui._note(box, "由城市管理者安排内政")
		return
	var row = ui._buttons(box)
	ui._btn(row, "调整税率", tax_dialog.bind(c))
	ui._btn(row, "赏赐民众", func(): ui._goods_dialog("赏赐民众", "从府库拨给民众。府库：" + Game.inv_text(c.treasury), {}, func(goods): send(c, "grant", {"goods": goods}, "赏赐民众")))
	ui._btn(row, "派工事", assignment_dialog.bind(c), int(c.idle_officials) == 0)

func officials(c: Dictionary, box: Node) -> void:
	ui._section(box, "士人")
	for shi in c.officials:
		ui._kv(box, str(shi.name), "可管理 %d 人 · 忠诚 %s" % [int(shi.capacity), Game.fx(shi.loyalty)])
		ui._note(box, "、".join(shi.skills.map(func(k): return Game.cname("skills", k))) + (" · 闲置" if shi.assignment == null else " · 正在主事"))
		if can_manage(c):
			var row = ui._buttons(box)
			ui._btn(row, "派工", assignment_dialog.bind(c, int(shi.id)), shi.assignment != null)
			ui._btn(row, "罢免", func(): ui._confirm("罢免%s，让他回到原来的人群？" % shi.name, func(): send(c, "dismiss_shi", {"shi": int(shi.id)}, "罢免士人")), shi.assignment != null)

## 展开状态按城、行业、工种与人群保存，服务器刷新后仍留在原处。
func fold(box: Node, key: String, text: String) -> VBoxContainer:
	var open: bool = _expanded.get(key, false)
	var button := Button.new()
	button.text = ("▾ " if open else "▸ ") + text
	button.alignment = HORIZONTAL_ALIGNMENT_LEFT
	button.toggle_mode = true
	button.button_pressed = open
	box.add_child(button)
	var margin := MarginContainer.new()
	margin.add_theme_constant_override("margin_left", 12)
	box.add_child(margin)
	var content := VBoxContainer.new()
	content.add_theme_constant_override("separation", 8)
	content.visible = open
	margin.add_child(content)
	margin.visible = open
	button.toggled.connect(func(on):
		_expanded[key] = on
		content.visible = on
		margin.visible = on
		button.text = ("▾ " if on else "▸ ") + text)
	return content

func population(c: Dictionary, box: Node) -> void:
	ui._section(box, "人口 · %d人" % int(c.population))
	ui._note(box, "兼业人口可出现在多个行业；行业人数已去重，不能相加。")
	var industries = Game.config.get("industries")
	if not industries is Array or industries.is_empty():
		ui._note(box, "服务器缺少行业配置，请重启服务器并重新连接。")
		return
	for industry in industries:
		var skills: Array = Game.config.skills.keys().filter(func(k): return Game.config.skills[k].get("industry", "") == industry.key)
		var groups: Array = c.groups.filter(func(g): return g.skills.any(func(k): return skills.has(k)))
		var total := 0
		for g in groups:
			total += int(g.size)
		var key := "%d/%s" % [int(c.id), industry.key]
		var category := fold(box, key, "%s · %d人" % [industry.name, total])
		for skill in skills:
			var members: Array = groups.filter(func(g): return g.skills.has(skill))
			if members.is_empty():
				continue
			members.sort_custom(func(a, b):
				if int(a.specialization) != int(b.specialization):
					return int(a.specialization) > int(b.specialization)
				if int(a.size) != int(b.size):
					return int(a.size) > int(b.size)
				return int(a.id) < int(b.id))
			var count := 0
			for g in members:
				count += int(g.size)
			var trade := str(Game.config.skills[skill].get("trade_name", Game.cname("skills", skill)))
			var trade_box := fold(category, key + "/" + str(skill), "%s · %d人" % [trade, count])
			for g in members:
				var group_box := fold(trade_box, key + "/%s/%d" % [skill, int(g.id)], "专精%s%% · %d人" % [Game.pct(g.specialization), int(g.size)])
				ui._note(group_box, "已派工%d · 未派工%d" % [int(g.managed), maxi(0, int(g.size) - int(g.managed))])
				ui._note(group_box, "愉悦%s · 教化%s · 忠诚%s" % [Game.fx(g.happiness), Game.fx(g.culture), Game.fx(g.loyalty)])
				if int(g.hostility) > 0:
					ui._note(group_box, "敌视%s" % Game.fx(g.hostility))
				for learning in c.learning:
					if int(learning.group) == int(g.id):
						ui._note(group_box, "学习%s · %d人 · %d回合" % [Game.cname("skills", learning.skill), int(learning.count), int(learning.turns)])
				if can_manage(c):
					var row = ui._buttons(group_box)
					ui._btn(row, "学习", people_dialog.bind(c, g, "learn_skill"))
					ui._btn(row, "调整技能", arrange_dialog.bind(c, g))
		if industry.key == str(Game.config.skills[Game.config.military.skill].industry):
			var units := fold(category, key + "/active", "现役军队")
			var found := false
			for army in Game.my_armies.values():
				if int(army.city) == int(c.id):
					found = true
					ui._note(units, "军队%d · %d人" % [int(army.id), Game.army_size(army)])
			if not found:
				ui._note(units, "无现役军队")

func number(box: Node, label: String, maximum: int, initial := 0) -> SpinBox:
	var n := SpinBox.new()
	n.max_value = maxi(0, maximum)
	n.value = initial
	ui._row(box, label, n)
	return n

func choice(box: Node, label: String, keys: Array, labels: Array) -> OptionButton:
	var opt := OptionButton.new()
	for i in range(keys.size()):
		opt.add_item(str(labels[i]))
		opt.set_item_metadata(i, keys[i])
	ui._row(box, label, opt)
	return opt

func selected(opt: OptionButton):
	return opt.get_item_metadata(opt.selected) if opt.selected >= 0 else null

func tax_dialog(c: Dictionary) -> void:
	var dv = ui._dialog("调整税率")
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	ui._note(v, "输入百分数，如 12.5 表示 12.5%%。上限 %s%%。收税还需士人主事收税工事。" % Game.fx(int(Game.config.economy.max_tax_rate) * 100))
	var edit := LineEdit.new()
	edit.text = Game.fx_input(int(c.tax_rate) * 100)
	ui._row(v, "税率 %", edit)
	d.confirmed.connect(func():
		var raw := Game.parse_fx(edit.text)
		if raw < 0 or raw % 100 != 0:
			ui._log("[color=#ff8080]税率请输入至多一位小数的非负百分数[/color]")
			return
		send(c, "set_tax_rate", {"rate": raw / 100}, "调整税率"))
	ui._popup(d)

func people_dialog(c: Dictionary, g: Dictionary, _kind: String) -> void:
	var dv = ui._dialog("学习技能")
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	ui._note(v, "%d人 · 专精%s%%" % [int(g.size), Game.pct(g.specialization)])
	var shi_skill := str(Game.config.shi.skill)
	var keys: Array = Game.config.skills.keys().filter(func(k): return k != shi_skill and not g.skills.has(k))
	# 学「士」沿用任用指令；已有士技能的人群也能从这里任用。
	keys.append(shi_skill)
	var skill := choice(v, "学习", keys, keys.map(func(k): return "%s（任用士人）" % Game.cname("skills", k) if k == shi_skill else Game.cname("skills", k)))
	var count := number(v, "人数", available_learning(c, g), 1)
	count.min_value = 1
	var explanation := Label.new()
	explanation.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	v.add_child(explanation)
	var refresh := func():
		var appoint: bool = selected(skill) == shi_skill
		var available := maxi(0, int(g.size) - int(g.managed)) if appoint else available_learning(c, g)
		count.max_value = maxi(1, available)
		d.get_ok_button().disabled = available <= 0
		explanation.text = "选中的人将成为士人，进入士人名单，可安排他们主持工事。" if appoint else "学习期间照常干活，学成后按新技能分组。"
	skill.item_selected.connect(func(_index): refresh.call())
	refresh.call()
	var drop_keys: Array = [""]
	drop_keys.append_array(g.skills)
	var drop := choice(v, "丢弃旧技能", drop_keys, drop_keys.map(func(k): return "不丢弃" if k == "" else Game.cname("skills", k)))
	ui._note(v, "技能槽上限 %d；满槽学习时需选一项旧技能丢弃。" % int(Game.config.population.max_skills))
	d.confirmed.connect(func():
		var appoint: bool = selected(skill) == shi_skill
		var fields := {"group": int(g.id), "count": int(count.value)}
		if not appoint:
			fields.skill = selected(skill)
		if selected(drop) != "":
			fields.drop_skill = selected(drop)
		send(c, "appoint_shi" if appoint else "learn_skill", fields, "任用士人" if appoint else "学习技能"))
	ui._popup(d)

func available_learning(c: Dictionary, g: Dictionary) -> int:
	var n := int(g.size)
	for learning in c.learning:
		if int(learning.group) == int(g.id):
			n -= int(learning.count)
	return maxi(0, n)

func arrange_dialog(c: Dictionary, g: Dictionary) -> void:
	var dv = ui._dialog("调整技能顺序")
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	var available := available_learning(c, g)
	var count := number(v, "人数", available, available)
	ui._note(v, "从上到下依次优先使用。可以移除技能，至少保留一项；正在学习的人需等学成。")
	var order: Array = g.skills.duplicate()
	var list := VBoxContainer.new()
	v.add_child(list)
	var refresh: Callable
	# 用信号重建列表，避免闭包循环引用自身。
	var rebuild := Button.new()
	rebuild.visible = false
	v.add_child(rebuild)
	refresh = func():
		ui._clear(list)
		for i in range(order.size()):
			var row := HBoxContainer.new()
			list.add_child(row)
			ui._note(row, Game.cname("skills", order[i]))
			ui._btn(row, "上移", func():
				var k = order[i]
				order[i] = order[i - 1]
				order[i - 1] = k
				rebuild.pressed.emit(), i == 0)
			ui._btn(row, "移除", func():
				order.remove_at(i)
				rebuild.pressed.emit(), order.size() <= 1)
	rebuild.pressed.connect(refresh)
	rebuild.pressed.emit()
	d.get_ok_button().disabled = available <= 0
	d.confirmed.connect(func(): send(c, "arrange_skills", {"group": int(g.id), "count": int(count.value), "skills": order}, "调整技能"))
	ui._popup(d)

## 每种候选工事都用服务器的 Task 格式保存，界面不自行推演规则。
func task_options(c: Dictionary) -> Array:
	var out := []
	for kind in ["collect_tax", "sacrifice", "repair_wall"]:
		out.append({"kind": kind})
	for key in Game.config.recipes:
		for mode in ["guan", "min"]:
			out.append({"kind": "produce", "recipe": key, "mode": mode})
	for tile in c.tiles:
		for key in Game.config.buildings:
			for mode in ["guan", "min"]:
				out.append({"kind": "construct", "building": key, "tile": int(tile.id), "mode": mode})
		for key in Game.config.improvements:
			out.append({"kind": "improve", "improvement": key, "tile": int(tile.id)})
	for h in Game.cells:
		var cell: Dictionary = Game.cells[h]
		if cell.city != null or not Game.vision.has(h):
			continue
		if not c.tiles.any(func(t): return Game.hex_distance(Game.hx(t.pos), h) == 1):
			continue
		if Game.config.terrains.get(cell.terrain, {}).get("reclaim_skill") == null:
			continue
		out.append({"kind": "reclaim", "at": Game.hd(h), "terrain": cell.terrain})
	return out

func assignment_dialog(c: Dictionary, shi_id := -1) -> void:
	var dv = ui._dialog("派工事")
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	var idle: Array = c.officials.filter(func(s): return s.assignment == null)
	var shi := choice(v, "主事士人", idle.map(func(s): return int(s.id)), idle.map(func(s): return "%s（管理 %d 人）" % [s.name, int(s.capacity)]))
	for i in range(shi.item_count):
		if int(shi.get_item_metadata(i)) == shi_id:
			shi.selected = i
	var tasks := task_options(c)
	var kinds := ["collect_tax", "sacrifice", "repair_wall", "produce", "construct", "improve", "reclaim"]
	var category := choice(v, "工事类型", kinds, ["收税", "祭祀", "修城墙", "生产", "营建", "改良", "开垦"])
	var task := choice(v, "具体工事", [], [])
	var info := Label.new()
	info.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	v.add_child(info)
	var workers := VBoxContainer.new()
	workers.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	v.add_child(workers)
	var inputs := {}
	var update_workers := func():
		ui._clear(workers)
		inputs.clear()
		var t = selected(task)
		if t == null:
			return
		var def: Dictionary = {}
		match str(t.kind):
			"produce": def = Game.config.recipes[t.recipe]
			"construct": def = Game.config.buildings[t.building]
			"improve": def = Game.config.improvements[t.improvement]
		var notes := []
		if def.get("skill") != null:
			notes.append("所需技能：" + Game.cname("skills", def.skill))
		if def.has("materials"):
			notes.append("材料：" + Game.inv_text(def.materials))
		if def.has("inputs"):
			notes.append("投入：" + Game.inv_text(def.inputs))
		info.text = "\n".join(notes)
		if t.kind == "sacrifice":
			ui._note(workers, "祭祀不投入劳力，消耗礼器等物资。")
			return
		for g in c.groups:
			var available := int(g.size) - int(g.managed)
			if t.kind == "collect_tax":
				available = int(g.size)
				for a in c.assignments:
					if a.task.kind == "collect_tax":
						available -= int(a.groups.get(str(int(g.id)), 0))
			inputs[str(int(g.id))] = number(workers, "人口%d · 专精%s%%" % [int(g.id), Game.pct(g.specialization)], available)
		ui._note(workers, "投入合计不能超过士人管理上限；收税覆盖人口，不占用劳力。")
	var update_tasks := func():
		task.clear()
		for t in tasks:
			if t.kind != selected(category):
				continue
			var label: String = Game.task_text(t)
			if t.has("tile"):
				for tile in c.tiles:
					if int(tile.id) == int(t.tile):
						label += " · " + Game.hex_label(Game.hx(tile.pos))
			if t.has("at"):
				label += " · " + Game.hex_label(Game.hx(t.at))
			task.add_item(label)
			task.set_item_metadata(task.item_count - 1, t)
		d.get_ok_button().disabled = idle.is_empty() or task.item_count == 0
		update_workers.call()
	category.item_selected.connect(func(_i): update_tasks.call())
	task.item_selected.connect(func(_i): update_workers.call())
	update_tasks.call()
	d.confirmed.connect(func():
		var counts := {}
		for key in inputs:
			var n := int(inputs[key].value)
			if n > 0:
				counts[key] = n
		send(c, "start_assignment", {"shi": selected(shi), "task": selected(task), "workers": counts}, "派工事"))
	ui._popup(d)
