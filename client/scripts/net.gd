## WebSocket 连接：收发 JSON 文本消息。只管通信，不懂游戏。
extends Node

signal opened
signal closed
signal message(msg: Dictionary)

var _ws := WebSocketPeer.new()
var _state := WebSocketPeer.STATE_CLOSED


func open(url: String) -> Error:
	_ws = WebSocketPeer.new()
	# welcome 里带着全部配置和两份报告，默认 64KB 的缓冲放不下
	_ws.inbound_buffer_size = 1 << 24
	_ws.outbound_buffer_size = 1 << 20
	_ws.max_queued_packets = 4096
	_state = WebSocketPeer.STATE_CLOSED
	return _ws.connect_to_url(url)


func close() -> void:
	_ws.close()


func is_open() -> bool:
	return _ws.get_ready_state() == WebSocketPeer.STATE_OPEN


func send(msg: Dictionary) -> void:
	if is_open():
		_ws.send_text(JSON.stringify(msg))


func _process(_delta: float) -> void:
	_ws.poll()
	var s := _ws.get_ready_state()
	if s != _state:
		_state = s
		if s == WebSocketPeer.STATE_OPEN:
			opened.emit()
		elif s == WebSocketPeer.STATE_CLOSED:
			closed.emit()
	while _ws.get_available_packet_count() > 0:
		var text := _ws.get_packet().get_string_from_utf8()
		var data = JSON.parse_string(text)
		if data is Dictionary:
			message.emit(data)
		else:
			push_warning("看不懂的消息：%s" % text.left(200))
