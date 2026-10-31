## 三维地图与二维标注共用轴向坐标；高度只用于显示，不参与规则。
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


var _viewport: SubViewport
var _world: Node3D
var _camera: Camera3D
var _focus := Vector3.ZERO
var _yaw := 0.0


func _ready() -> void:
	clip_contents = true
	mouse_filter = Control.MOUSE_FILTER_STOP
	var container := SubViewportContainer.new()
	container.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	container.stretch = true
	container.show_behind_parent = true
	container.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(container)
	_viewport = SubViewport.new()
	_viewport.own_world_3d = true
	_viewport.transparent_bg = false
	container.add_child(_viewport)
	_world = Node3D.new()
	_viewport.add_child(_world)
	_camera = Camera3D.new()
	_camera.projection = Camera3D.PROJECTION_ORTHOGONAL
	_camera.far = 10000
	_viewport.add_child(_camera)
	var environment := WorldEnvironment.new()
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color("#202c33")
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color("#dce7e9")
	env.ambient_light_energy = 0.7
	environment.environment = env
	_viewport.add_child(environment)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-55, -30, 0)
	sun.light_energy = 1.2
	_viewport.add_child(sun)
	Game.view_changed.connect(_on_view_changed)
	resized.connect(center_map)
	_on_view_changed()


func _height(h: Vector2i) -> float:
	var terrain := str(Game.cells.get(h, {}).get("terrain", ""))
	# 仅为地形表现高度，无游戏数值含义。
	return float({"mountain": 25, "forest": 7, "water": -3, "river": -3}.get(terrain, 1))


func _point(p: Vector2, height: float) -> Vector3:
	return Vector3(p.x, height, p.y)


func _material(color: Color) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.95
	return material


func _mesh(mesh: Mesh, at: Vector3, color: Color) -> MeshInstance3D:
	var instance := MeshInstance3D.new()
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = at
	_world.add_child(instance)
	return instance


func _box(at: Vector3, dimensions: Vector3, color: Color) -> void:
	var mesh := BoxMesh.new()
	mesh.size = dimensions
	_mesh(mesh, at, color)


func _on_view_changed() -> void:
	if _world == null:
		return
	for child in _world.get_children():
		_world.remove_child(child)
		child.queue_free()
	for h in Game.cells:
		var terrain := str(Game.cells[h].terrain)
		var color: Color = TERRAIN_COLORS.get(terrain, Color.GRAY)
		if Game.tile_city.has(h):
			color = color.lerp(Game.person_color(Game.city_owner(Game.tile_city[h])), 0.15)
		if not Game.vision.has(h):
			color = color.darkened(0.55)
		var height := _height(h)
		var tile := CylinderMesh.new()
		tile.top_radius = HEX * 0.985
		tile.bottom_radius = tile.top_radius
		tile.height = height + 16
		tile.radial_segments = 6
		_mesh(tile, _point(hex_to_px(h), (height - 16) / 2), color)
		if terrain == "forest":
			for offset in [Vector2(-12, 3), Vector2(10, -8), Vector2(6, 15)]:
				var tree := CylinderMesh.new()
				tree.top_radius = 0
				tree.bottom_radius = 10
				tree.height = 23
				tree.radial_segments = 5
				_mesh(tree, _point(hex_to_px(h) + offset, height + 11), color.darkened(0.22))
	for h in Game.seat_city:
		var id: int = Game.seat_city[h]
		var base := _point(hex_to_px(h), _height(h))
		var color := Game.person_color(Game.city_owner(id))
		if not Game.vision.has(h):
			color = color.darkened(0.5)
		_box(base + Vector3(0, 5, 0), Vector3(25, 10, 20), Color("#c8ba9b").lerp(color, 0.25))
		_box(base + Vector3(0, 12, 0), Vector3(29, 4, 23), color)
		var city = Game.my_cities.get(id, Game.foreign_cities.get(id))
		# 己方 wall 是剩余血量，外城 wall 是布尔值，不能混用比较。
		var standing_wall := false
		if city != null:
			if Game.my_cities.has(id):
				standing_wall = city.wall != null and int(city.wall) > 0
			else:
				standing_wall = bool(city.wall) and not bool(city.wall_broken)
		if standing_wall:
			for side in [-1, 1]:
				_box(base + Vector3(side * 21, 5, 0), Vector3(4, 10, 44), Color("#8b806b"))
				_box(base + Vector3(0, 5, side * 21), Vector3(44, 10, 4), Color("#8b806b"))
		var occupier = Game.city_occupier(id)
		_flag(base + Vector3(14, 0, -12), Game.person_color(occupier) if occupier != null else color)
	for h in Game.armies_at:
		var entries: Array = Game.armies_at[h]
		for i in entries.size():
			var entry: Dictionary = entries[i]
			var color := Game.person_color(entry.data.commander)
			if not entry.own and not entry.data.visible:
				color = color.darkened(0.55)
			_flag(_point(_army_pos(h, i, entries.size()), _height(h)), color)
	if not _centered:
		center_map()
	_update_camera()


func _flag(base: Vector3, color: Color) -> void:
	_box(base + Vector3(0, 13, 0), Vector3(1.5, 26, 1.5), Color("#46382a"))
	_box(base + Vector3(6, 22, 0), Vector3(12, 8, 1), color)


func _update_camera() -> void:
	if _camera == null or size.y < 1:
		return
	_camera.size = size.y / zoom
	_camera.position = _focus + Vector3(sin(_yaw) * 1000, 1100, cos(_yaw) * 1000)
	_camera.look_at(_focus)
	queue_redraw()


func center_map() -> void:
	if Game.cells.is_empty() or size.x < 10:
		return
	var bounds := Rect2(hex_to_px(Game.cells.keys()[0]), Vector2.ZERO)
	for h in Game.cells:
		bounds = bounds.expand(hex_to_px(h))
	bounds = bounds.grow(HEX * 2)
	_focus = _point(bounds.get_center(), 0)
	zoom = clampf(minf(size.x / bounds.size.x, size.y / bounds.size.y), 0.3, 2.0)
	_centered = true
	_update_camera()


func center_on(h: Vector2i) -> void:
	_focus = _point(hex_to_px(h), 0)
	_update_camera()


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
	return _camera.unproject_position(_point(world, _height(px_to_hex(world)) + 2))


## 按每块地形表面求交，避免山地的点选被平面投影偏移。
func to_world(screen: Vector2) -> Vector2:
	var ray := _camera.project_ray_origin(screen)
	var direction := _camera.project_ray_normal(screen)
	var nearest := INF
	var result := Vector2.ZERO
	for h in Game.cells:
		var distance: float = (_height(h) - ray.y) / direction.y
		var hit := ray + direction * distance
		if distance > 0 and distance < nearest and px_to_hex(Vector2(hit.x, hit.z)) == h:
			nearest = distance
			result = Vector2(hit.x, hit.z)
	if nearest == INF:
		var hit := ray + direction * (-ray.y / direction.y)
		result = Vector2(hit.x, hit.z)
	return result


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
			if mm.alt_pressed:
				_yaw -= mm.relative.x * 0.008
			else:
				_pan(mm.relative)
			_update_camera()
			queue_redraw()
		accept_event()
	elif event is InputEventMagnifyGesture:
		_zoom_at((event as InputEventMagnifyGesture).position, (event as InputEventMagnifyGesture).factor)
	elif event is InputEventPanGesture:
		_pan(-(event as InputEventPanGesture).delta * 10)
		_update_camera()
		queue_redraw()


func _pan(delta: Vector2) -> void:
	var right := _camera.global_transform.basis.x
	var forward := Vector3(-right.z, 0, right.x)
	_focus -= (right * delta.x + forward * delta.y * 1.5) / zoom


func _zoom_at(at: Vector2, factor: float) -> void:
	var before := to_world(at)
	zoom = clampf(zoom * factor, 0.3, 3.0)
	_update_camera()
	var after := to_world(at)
	_focus += _point(before - after, 0)
	_update_camera()


func _outline(h: Vector2i, color: Color, width: float = 2.0) -> void:
	var points := PackedVector2Array()
	for corner in corners(hex_to_px(h), HEX * 0.95):
		points.append(_camera.unproject_position(_point(corner, _height(h) + 1)))
	points.append(points[0])
	draw_polyline(points, color, width, true)


func _draw() -> void:
	if _camera == null or Game.cells.is_empty():
		return
	for h in Game.tile_city:
		var id: int = Game.tile_city[h]
		var points := corners(hex_to_px(h))
		for i in 6:
			if Game.tile_city.get(h + DIRS[i], -1) != id:
				draw_line(_camera.unproject_position(_point(points[i], _height(h) + 1)), _camera.unproject_position(_point(points[(i + 1) % 6], _height(h) + 1)), Game.person_color(Game.city_owner(id)), 2, true)
	for h in highlight:
		_outline(h, highlight[h])
	if selected_hex != null:
		_outline(selected_hex, Color("#ffe14d"), 3)
	var army = Game.my_armies.get(selected_army)
	if army != null:
		if army.supply != null and not army.supply.path.is_empty():
			var seat = Game.my_cities.get(int(army.supply.from))
			var previous := to_screen(hex_to_px(Game.hx(seat.seat if seat != null else army.supply.path[0])))
			for p in army.supply.path:
				var current := to_screen(hex_to_px(Game.hx(p)))
				draw_dashed_line(previous, current, Color("#f3e3a0"), 2, 6)
				previous = current
		if army.dest != null:
			draw_line(to_screen(hex_to_px(Game.hx(army.pos))), to_screen(hex_to_px(Game.hx(army.dest))), Color.WHITE, 3, true)
	var font := get_theme_default_font()
	for h in Game.seat_city:
		_text(font, to_screen(hex_to_px(h)) + Vector2(0, 29), Game.city_name(Game.seat_city[h]), 17, Color.WHITE, true)
	for h in Game.armies_at:
		var entries: Array = Game.armies_at[h]
		for i in entries.size():
			var entry: Dictionary = entries[i]
			var label := str(Game.army_size(entry.data))
			if not entry.own and not entry.data.visible:
				label += "?"
			var at := to_screen(_army_pos(h, i, entries.size()))
			if int(entry.data.id) == selected_army:
				draw_arc(at, 15, 0, TAU, 24, Color("#ffe14d"), 2)
			_text(font, at + Vector2(0, 6), label, 14, Color.WHITE, true)


func _text(font: Font, at: Vector2, s: String, fs: int, col: Color, outline := false) -> void:
	var w := 200.0
	var pos := at - Vector2(w / 2, 0)
	if outline:
		draw_string_outline(font, pos, s, HORIZONTAL_ALIGNMENT_CENTER, w, fs, 4, Color(0, 0, 0, 0.8))
	draw_string(font, pos, s, HORIZONTAL_ALIGNMENT_CENTER, w, fs, col)


func _army_pos(h: Vector2i, i: int, n: int) -> Vector2:
	var c := hex_to_px(h)
	# 城的治所上的军队挪到右上角，别挡住城
	if Game.seat_city.has(h):
		c += Vector2(HEX * 0.45, -HEX * 0.45)
	if n > 1:
		c += Vector2((i - (n - 1) / 2.0) * HEX * 0.55, 0)
	return c


## 这一格上的军队，自己的在前
func armies_on(h: Vector2i) -> Array:
	var list: Array = Game.armies_at.get(h, []).duplicate()
	list.sort_custom(func(x, y): return x.own and not y.own)
	return list
