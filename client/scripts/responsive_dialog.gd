## 表单内容独立滚动，确认按钮固定在底部；弹窗始终适配当前游戏窗口。
extends ConfirmationDialog

@onready var scroll: ScrollContainer = $Scroll
@onready var content: VBoxContainer = $Scroll/Content
var _fitting := false

func _ready() -> void:
	wrap_controls = false
	content.minimum_size_changed.connect(_schedule_fit)
	get_tree().root.size_changed.connect(_schedule_fit)
	visibility_changed.connect(func(): if visible: _schedule_fit())

func _schedule_fit() -> void:
	if not visible or _fitting:
		return
	_fitting = true
	_fit.call_deferred()

func _prepare(node: Node) -> void:
	if node is OptionButton:
		node.fit_to_longest_item = false
		node.clip_text = true
		node.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		if not node.has_meta("dialog_tooltip"):
			node.set_meta("dialog_tooltip", true)
			node.item_selected.connect(func(index): node.tooltip_text = node.get_item_text(index))
		if node.selected >= 0:
			node.tooltip_text = node.get_item_text(node.selected)
	for child in node.get_children():
		_prepare(child)

func _fit() -> void:
	if not is_instance_valid(content) or is_queued_for_deletion():
		return
	_prepare(content)
	var screen := get_tree().root.get_visible_rect().size
	var limit := Vector2i(screen * 0.85)
	var width := mini(640, limit.x)
	# 横向最小尺寸不跟随表单内容，防止长选项把窗口撑出屏幕。
	scroll.custom_minimum_size = Vector2.ZERO
	min_size = Vector2i.ZERO
	max_size = limit
	size = Vector2i(width, mini(size.y, limit.y))
	await get_tree().process_frame
	if not is_instance_valid(self) or is_queued_for_deletion():
		return
	var footer := int(get_ok_button().get_combined_minimum_size().y) + 48
	var height := mini(int(content.get_combined_minimum_size().y), maxi(0, limit.y - footer))
	scroll.custom_minimum_size.y = height
	size = Vector2i(width, mini(height + footer, limit.y))
	move_to_center()
	_fitting = false
