import type { components } from './api/openapi';
import { validateWireValue, type SettingsSnapshot, type StateSnapshot } from './wire';

type SettingsReadValues = components['schemas']['SettingsReadValues'];

export function settingsValues(
  overrides: Partial<SettingsReadValues> = {}
): SettingsReadValues {
  const values: SettingsReadValues = {
    active_profile: 'default',
    auto_reload_config: false,
    'camera.capture_fps': 60,
    'camera.capture_resolution': '1280x720',
    'camera.device': 0,
    'camera.flip_mode': 'none',
    'camera.screenshot_format': 'png',
    'commands.tag_match_mode': 'exact',
    'dynamic.callback_hard_timeout_ms': 5000,
    'dynamic.callback_max_concurrency': 8,
    'dynamic.callback_queue_capacity': 1024,
    'dynamic.callback_soft_timeout_grace_ms': 1000,
    'dynamic.callback_soft_timeout_ms': 2000,
    'input.keyboard_enabled': true,
    'input.left_stick_mouse_enabled': false,
    'input.right_stick_mouse_enabled': false,
    'input.touchscreen_area': { bottom: 1, left: 0, right: 1, top: 0 },
    jpeg_quality: 85,
    language: 'ja',
    'notifications.discord.avatar_url': '',
    'notifications.discord.on_script_end': false,
    'notifications.discord.on_script_start': false,
    'notifications.discord.username': '',
    'notifications.discord.webhook_url': { configured: false },
    'notifications.line_menu_behavior': 'message',
    'notifications.windows.on_script_end': false,
    'notifications.windows.on_script_start': false,
    'python.script.packages.list': [],
    'python.script.packages.override_application_constraints': false,
    'python.script.packages.override_package_metadata_constraints': false,
    'python.script.packages.revalidate_mutable_sources': false,
    'python.script.packages.uv_config': null,
    'python.script.shutdown_timeout_ms': 2000,
    'python.script.venv': '/tmp/pokecon-python',
    report_ignored_profile_global_settings: true,
    'serial.baud_rate': 9600,
    'serial.data_format': 'default',
    'serial.port': '',
    'server.bind_address': '127.0.0.1',
    'server.port': 8020,
    'server.web_dir': '/tmp/pokecon-web',
    'shortcuts.button_1': '',
    'shortcuts.button_10': '',
    'shortcuts.button_2': '',
    'shortcuts.button_3': '',
    'shortcuts.button_4': '',
    'shortcuts.button_5': '',
    'shortcuts.button_6': '',
    'shortcuts.button_7': '',
    'shortcuts.button_8': '',
    'shortcuts.button_9': '',
    stun_server: '',
    'ui.camera.guide_visible': false,
    'ui.camera.live_view_enabled': true,
    'ui.camera.pixel_values_visible': false,
    'ui.controller_position': 'bottom',
    'ui.desktop.close_behavior': 'ask',
    'ui.desktop.disable_compositing': false,
    'ui.dialog_button_position': 'bottom',
    'ui.fps': 30,
    'ui.fps_options': [5, 15, 30, 60],
    'ui.output_split_ratio': 20,
    'ui.stdout_destination': 'output_1',
    'ui.widget_mode': 'all',
    'webrtc.auto_recover': true,
    'webrtc.recovery_probe_interval_sec': 30,
    'websocket.ping_interval_sec': 15,
    'websocket.pong_timeout_sec': 10,
    'websocket.reconnect_interval_sec': 3,
    'websocket.reconnect_max_retries': 20,
    ...overrides,
    'input.allow_manual_intervention': overrides['input.allow_manual_intervention'] ?? true
  };
  return validateWireValue('SettingsReadValues', values) as SettingsReadValues;
}

export function settingsSnapshot(
  revision: string,
  overrides: Partial<SettingsReadValues> = {}
): SettingsSnapshot {
  return validateWireValue('SettingsSnapshot', {
    apply_failures: {},
    pending_restart_values: {},
    restart_required: [],
    revision,
    values: settingsValues(overrides)
  }) as SettingsSnapshot;
}

export function stateSnapshot(
  revision: string,
  overrides: Partial<StateSnapshot> = {}
): StateSnapshot {
  return validateWireValue('StateSnapshot', {
    active_profile: 'default',
    available_profiles: ['default'],
    camera_device: 0,
    camera_fps: 0,
    camera_opened: false,
    camera_resolution: '1280x720',
    command_candidates: [],
    command_display_cache_loading: false,
    command_display_lists: { '-': [] },
    command_state: 'stopped',
    current_command: null,
    holding_buttons: [],
    is_running: false,
    last_input: null,
    pending_profile: null,
    pid: 1234,
    revision,
    serial_baud_rate: 9600,
    serial_connected: false,
    serial_port: null,
    tags: [],
    ...overrides
  }) as StateSnapshot;
}
