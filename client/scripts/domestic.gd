## 内政界面：仅组织输入，规则校验和结算交给服务器。
extends RefCounted

var ui

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
			ui._btn(row, "派工·%s" % shi.name, assignment_dialog.bind(c, int(shi.id)), shi.assignment != null)
			ui._btn(row, "罢免·%s" % shi.name, func(): ui._confirm("罢免%s，让他回到原来的人群？" % shi.name, func(): send(c, "dismiss_shi", {"shi": int(shi.id)}, "罢免士人")), shi.assignment != null)

func group_actions(c: Dictionary, g: Dictionary, box: Node) -> void:
	ui._note(box, "已派工 %d 人 · 未派工 %d 人" % [int(g.managed), maxi(0, int(g.size) - int(g.managed))])
	if not can_manage(c):
		return
	var row = ui._buttons(box)
	ui._btn(row, "任用士人·%s" % g.name, people_dialog.bind(c, g, "appoint_shi"))
	ui._btn(row, "学技能·%s" % g.name, people_dialog.bind(c, g, "learn_skill"))
	ui._btn(row, "调技能·%s" % g.name, arrange_dialog.bind(c, g))

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

func people_dialog(c: Dictionary, g: Dictionary, kind: String) -> void:
	var appoint := kind == "appoint_shi"
	var dv = ui._dialog("任用士人" if appoint else "学习技能")
	var d: ConfirmationDialog = dv[0]
	var v: VBoxContainer = dv[1]
	ui._note(v, "%s：%s" % [g.name, "、".join(g.skills.map(func(k): return Game.cname("skills", k)))])
	var available := int(g.size) - int(g.managed) if appoint else available_learning(c, g)
	var count := number(v, "人数", available, mini(1, available))
	var skill: OptionButton
	if not appoint:
		var keys: Array = Game.config.skills.keys().filter(func(k): return not g.skills.has(k))
		skill = choice(v, "学习", keys, keys.map(func(k): return Game.cname("skills", k)))
		ui._note(v, "学习期间照常干活，学成后按新技能分组。")
	var drop_keys: Array = [""]
	drop_keys.append_array(g.skills)
	var drop := choice(v, "丢弃旧技能", drop_keys, drop_keys.map(func(k): return "不丢弃" if k == "" else Game.cname("skills", k)))
	ui._note(v, "技能槽上限 %d；满槽学习时需选一项旧技能丢弃。" % int(Game.config.population.max_skills))
	d.get_ok_button().disabled = available <= 0 or (not appoint and skill.item_count == 0)
	d.confirmed.connect(func():
		var fields := {"group": int(g.id), "count": int(count.value)}
		if not appoint:
			fields.skill = selected(skill)
		if selected(drop) != "":
			fields.drop_skill = selected(drop)
		send(c, kind, fields, "任用士人" if appoint else "学习技能"))
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
	var scroll := ScrollContainer.new()
	scroll.custom_minimum_size.y = 180
	v.add_child(scroll)
	var workers := VBoxContainer.new()
	workers.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.add_child(workers)
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
			inputs[str(int(g.id))] = number(workers, "%s（%s）" % [g.name, "、".join(g.skills.map(func(k): return Game.cname("skills", k)))], available)
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
