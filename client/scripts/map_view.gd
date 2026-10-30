## 地图：六边形按地形上色，城画图形加城名，军队画圆标加人数，视野外的格子变暗。
## 左键选中，右键发给 main 处理（行军），拖动平移，滚轮缩放。
extends Control

signal hex_clicked(h: Vector2i)
signal hex_right_clicked(h: Vector2i)

const HEX := 40.0
const SQ3 := 1.7320508

## 地形的颜色只是显示用的，名称和数值以配置为准
const TERRAIN_COLORS := {
	"farmland": Color("#cdb963"),
	"water": Color("#6a9cbd"),
	"forest": Color("#4b7f3f"),
	"salt_pond": Color("#ddd8cb"),
	"copper_mine": Color("#a86e42"),
	"jade_mine": Color("#79b3a0"),
	"pasture": Color("#9cbf62"),
	"mountain": Color("#7f776c"),
	"river": Color("#4677ad"),
}

var zoom := 1.0
var origin := Vector2.ZERO  # 世界原点在控件里的位置
## 选中的格子；没有选中为 null
var selected_hex = null
## 选中的军队：画行军路线和粮道
var selected_army := -1
## 额外高亮的格子（攻击范围等）：格子 → 颜色
var highlight := {}

var _dragging := false
var _drag_moved := false
var _drag_start := Vector2.ZERO
var _centered := false


func _ready() -> void:
	clip_contents = true
	mouse_filter = Control.MOUSE_FILTER_STOP
	Game.view_changed.connect(_on_view_changed)
	resized.connect(func(): if not _centered: center_map())


func _on_view_changed() -> void:
	if not _centered:
		center_map()
	queue_redraw()


func center_map() -> void:
	if Game.cells.is_empty() or size.x < 10:
		return
	var box := Rect2(hex_to_px(Game.cells.keys()[0]), Vector2.ZERO)
	for h in Game.cells:
		box = box.expand(hex_to_px(h))
	box = box.grow(HEX)
	zoom = clampf(minf(size.x / box.size.x, size.y / box.size.y), 0.4, 2.0)
	origin = size / 2 - box.get_center() * zoom
	_centered = true
	queue_redraw()


func center_on(h: Vector2i) -> void:
	origin = size / 2 - hex_to_px(h) * zoom
	queue_redraw()


# ---------------------------------------------------------------- 坐标换算

static func hex_to_px(h: Vector2i) -> Vector2:
	return Vector2(HEX * SQ3 * (h.x + h.y / 2.0), HEX * 1.5 * h.y)


static func px_to_hex(p: Vector2) -> Vector2i:
	var q := (SQ3 / 3.0 * p.x - p.y / 3.0) / HEX
	var r := (2.0 / 3.0 * p.y) / HEX
	var s := -q - r
	var rq := roundf(q)
	var rr := roundf(r)
	var rs := roundf(s)
	var dq := absf(rq - q)
	var dr := absf(rr - r)
	var ds := absf(rs - s)
	if dq > dr and dq > ds:
		rq = -rr - rs
	elif dr > ds:
		rr = -rq - rs
	return Vector2i(int(rq), int(rr))


func to_screen(world: Vector2) -> Vector2:
	return origin + world * zoom


func to_world(screen: Vector2) -> Vector2:
	return (screen - origin) / zoom


## 六个角，第 i 个角在 60i-30 度（尖顶朝上）。第 i 条边（角 i 到 i+1）对着 DIRS[i] 方向的邻格
const DIRS := [Vector2i(1, 0), Vector2i(0, 1), Vector2i(-1, 1), Vector2i(-1, 0), Vector2i(0, -1), Vector2i(1, -1)]


static func corners(c: Vector2, radius := HEX) -> PackedVector2Array:
	var pts := PackedVector2Array()
	for i in 6:
		var a := deg_to_rad(60.0 * i - 30.0)
		pts.append(c + Vector2(cos(a), sin(a)) * radius)
	return pts


# ---------------------------------------------------------------- 输入

func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_WHEEL_UP and mb.pressed:
			_zoom_at(mb.position, 1.15)
		elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN and mb.pressed:
			_zoom_at(mb.position, 1.0 / 1.15)
		elif mb.button_index == MOUSE_BUTTON_LEFT or mb.button_index == MOUSE_BUTTON_MIDDLE:
			if mb.pressed:
				_dragging = true
				_drag_moved = false
				_drag_start = mb.position
			else:
				_dragging = false
				if not _drag_moved and mb.button_index == MOUSE_BUTTON_LEFT:
					hex_clicked.emit(px_to_hex(to_world(mb.position)))
		elif mb.button_index == MOUSE_BUTTON_RIGHT and mb.pressed:
			hex_right_clicked.emit(px_to_hex(to_world(mb.position)))
		accept_event()
	elif event is InputEventMouseMotion and _dragging:
		var mm := event as InputEventMouseMotion
		if _drag_moved or mm.position.distance_to(_drag_start) > 5:
			_drag_moved = true
			origin += mm.relative
			queue_redraw()
		accept_event()
	elif event is InputEventMagnifyGesture:
		_zoom_at((event as InputEventMagnifyGesture).position, (event as InputEventMagnifyGesture).factor)
	elif event is InputEventPanGesture:
		origin -= (event as InputEventPanGesture).delta * 10
		queue_redraw()


func _zoom_at(at: Vector2, factor: float) -> void:
	var w := to_world(at)
	zoom = clampf(zoom * factor, 0.3, 3.0)
	origin = at - w * zoom
	queue_redraw()


# ---------------------------------------------------------------- 绘制

func _draw() -> void:
	draw_rect(Rect2(Vector2.ZERO, size), Color("#1d1f24"))
	if Game.cells.is_empty():
		return
	draw_set_transform(origin, 0, Vector2(zoom, zoom))
	var font := get_theme_default_font()

	# 地形
	for h in Game.cells:
		var c: Dictionary = Game.cells[h]
		var col: Color = TERRAIN_COLORS.get(str(c.terrain), Color(0.6, 0.6, 0.6))
		draw_colored_polygon(corners(hex_to_px(h)), col)

	# 城的疆域：按所有者上色
	for h in Game.tile_city:
		var lord = Game.city_owner(Game.tile_city[h])
		var col := Game.person_color(lord)
		col.a = 0.22
		draw_colored_polygon(corners(hex_to_px(h), HEX * 0.98), col)

	# 视野外变暗
	for h in Game.cells:
		if not Game.vision.has(h):
			draw_colored_polygon(corners(hex_to_px(h)), Color(0.05, 0.05, 0.08, 0.5))

	# 格线
	for h in Game.cells:
		var pts := corners(hex_to_px(h))
		pts.append(pts[0])
		draw_polyline(pts, Color(0, 0, 0, 0.25), 1.0)

	# 城与城、城与野地之间的边界
	for h in Game.tile_city:
		var id: int = Game.tile_city[h]
		var col := Game.person_color(Game.city_owner(id)).darkened(0.2)
		var pts := corners(hex_to_px(h))
		for i in 6:
			var n: Vector2i = h + DIRS[i]
			if Game.tile_city.get(n, -1) != id:
				draw_line(pts[i], pts[(i + 1) % 6], col, 3.0)

	# 高亮
	for h in highlight:
		var pts := corners(hex_to_px(h), HEX * 0.92)
		pts.append(pts[0])
		draw_polyline(pts, highlight[h], 3.0)

	# 选中的军队：行军目的地、粮道
	var army = Game.my_armies.get(selected_army)
	if army != null:
		var from := hex_to_px(Game.hx(army.pos))
		if army.supply != null and not army.supply.path.is_empty():
			var seat = Game.my_cities.get(int(army.supply.from))
			var prev := hex_to_px(Game.hx(seat.seat)) if seat != null else hex_to_px(Game.hx(army.supply.path[0]))
			for p in army.supply.path:
				var cur := hex_to_px(Game.hx(p))
				draw_dashed_line(prev, cur, Color("#f3e3a0"), 2.5, 6.0)
				prev = cur
		if army.dest != null:
			var to := hex_to_px(Game.hx(army.dest))
			draw_line(from, to, Color(1, 1, 1, 0.85), 3.0)
			draw_circle(to, 6.0, Color(1, 1, 1, 0.85))

	# 选中的格子
	if selected_hex != null:
		var pts := corners(hex_to_px(selected_hex), HEX * 0.95)
		pts.append(pts[0])
		draw_polyline(pts, Color("#ffe14d"), 3.5)

	# 城
	for h in Game.seat_city:
		_draw_city(h, Game.seat_city[h])

	# 军队
	for h in Game.armies_at:
		var list: Array = Game.armies_at[h]
		for i in list.size():
			_draw_army(h, list[i], i, list.size())

	# 文字在屏幕坐标里画，缩放时不糊
	draw_set_transform(Vector2.ZERO, 0, Vector2.ONE)
	var fs := int(clampf(14 * zoom, 9, 24))
	if zoom > 0.7:
		for h in Game.cells:
			if Game.seat_city.has(h) or Game.armies_at.has(h):
				continue
			var sym := str(Game.config.get("terrains", {}).get(str(Game.cells[h].terrain), {}).get("symbol", ""))
			_text(font, to_screen(hex_to_px(h)) + Vector2(0, fs * 0.35), sym, fs, Color(0, 0, 0, 0.35))
	for h in Game.seat_city:
		var id: int = Game.seat_city[h]
		var p := to_screen(hex_to_px(h) + Vector2(0, HEX * 0.82))
		_text(font, p, Game.city_name(id), fs + 2, Color.WHITE, true)
	for h in Game.armies_at:
		var list: Array = Game.armies_at[h]
		for i in list.size():
			var c := to_screen(_army_pos(h, i, list.size()))
			var a: Dictionary = list[i].data
			var label := str(Game.army_size(a))
			if not list[i].own and not a.visible:
				label += "?"
			_text(font, c + Vector2(0, fs * 0.36), label, maxi(fs - 2, 9), Color.WHITE, true)


func _text(font: Font, at: Vector2, s: String, fs: int, col: Color, outline := false) -> void:
	var w := 200.0
	var pos := at - Vector2(w / 2, 0)
	if outline:
		draw_string_outline(font, pos, s, HORIZONTAL_ALIGNMENT_CENTER, w, fs, 4, Color(0, 0, 0, 0.8))
	draw_string(font, pos, s, HORIZONTAL_ALIGNMENT_CENTER, w, fs, col)


func _draw_city(h: Vector2i, id: int) -> void:
	var c := hex_to_px(h) + Vector2(0, -4)
	var city = Game.my_cities.get(id, Game.foreign_cities.get(id))
	var owner_col := Game.person_color(Game.city_owner(id))
	var has_wall := false
	if city != null:
		# 自己的城报告里 wall 是剩下的防御值，别人的城是有没有城墙
		has_wall = city.wall != null and city.wall != false
	# 城：方形城郭，有城墙的画外圈
	var r := HEX * 0.42
	if has_wall:
		draw_rect(Rect2(c - Vector2(r + 6, r + 6), Vector2(r + 6, r + 6) * 2), Color("#3b3026"), false, 3.0)
	draw_rect(Rect2(c - Vector2(r, r), Vector2(r, r) * 2), Color("#e9dfc8"))
	draw_rect(Rect2(c - Vector2(r, r), Vector2(r, r) * 2), owner_col.darkened(0.3), false, 3.0)
	# 城门和宫室
	draw_rect(Rect2(c + Vector2(-r * 0.25, r * 0.4), Vector2(r * 0.5, r * 0.6)), Color("#5a4632"))
	draw_rect(Rect2(c + Vector2(-r * 0.5, -r * 0.6), Vector2(r, r * 0.6)), owner_col)
	# 被攻占：插上占领者的旗
	var occ = Game.city_occupier(id)
	if occ != null:
		var pole := c + Vector2(r * 0.9, -r * 0.9)
		draw_line(pole, pole + Vector2(0, -r * 1.2), Color.BLACK, 2.0)
		draw_colored_polygon(PackedVector2Array([pole + Vector2(0, -r * 1.2), pole + Vector2(r * 0.9, -r * 0.95), pole + Vector2(0, -r * 0.7)]), Game.person_color(occ))


func _army_pos(h: Vector2i, i: int, n: int) -> Vector2:
	var c := hex_to_px(h)
	# 城的治所上的军队挪到右上角，别挡住城
	if Game.seat_city.has(h):
		c += Vector2(HEX * 0.45, -HEX * 0.45)
	if n > 1:
		c += Vector2((i - (n - 1) / 2.0) * HEX * 0.55, 0)
	return c


func _draw_army(h: Vector2i, entry: Dictionary, i: int, n: int) -> void:
	var a: Dictionary = entry.data
	var c := _army_pos(h, i, n)
	var col := Game.person_color(a.commander)
	var faded: bool = not entry.own and not a.visible
	if faded:
		col.a = 0.45
	var rad := HEX * 0.3
	# 旗杆和三角旗
	draw_line(c + Vector2(-rad * 0.2, -rad), c + Vector2(-rad * 0.2, -rad * 2.1), Color(0, 0, 0, col.a), 2.0)
	draw_colored_polygon(PackedVector2Array([c + Vector2(-rad * 0.2, -rad * 2.1), c + Vector2(rad * 0.9, -rad * 1.75), c + Vector2(-rad * 0.2, -rad * 1.4)]), col)
	draw_circle(c, rad, col)
	var ring := Color.WHITE if entry.own else Color(0.1, 0.1, 0.1, col.a)
	draw_arc(c, rad, 0, TAU, 24, ring, 2.5 if int(a.id) == selected_army else 1.5)
	if int(a.id) == selected_army:
		draw_arc(c, rad + 4, 0, TAU, 24, Color("#ffe14d"), 2.5)


## 这一格上的军队，自己的在前
func armies_on(h: Vector2i) -> Array:
	var list: Array = Game.armies_at.get(h, []).duplicate()
	list.sort_custom(func(x, y): return x.own and not y.own)
	return list
