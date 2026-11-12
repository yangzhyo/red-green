#!/usr/bin/env python3
"""使用真实 LVGL 帧验证 Metro 布局、进度条及整个焦点色块的闪烁。"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

preview = Path(sys.argv[1]).resolve()
destination = Path(sys.argv[2]) if len(sys.argv) > 2 else None
fixture = json.loads(Path(__file__).with_name('five.json').read_text())


def render(directory, name, model, end):
    source, output = directory / f'{name}.json', directory / f'{name}.ppm'
    source.write_text(json.dumps(model, ensure_ascii=False))
    result = subprocess.run([str(preview), str(source), str(output), str(end)],
                            check=True, capture_output=True, text=True)
    header, pixels = output.read_bytes().split(b'\n255\n', 1)
    assert header == b'P6\n480 480'
    def pixel(x, y):
        return tuple(pixels[(y * 480 + x) * 3:(y * 480 + x) * 3 + 3])
    pixel.labels = json.loads(next(line for line in result.stdout.splitlines() if line.startswith('[')))
    pixel.hero = next(label['text'] for label in pixel.labels if label['x'] == 24 and label['y'] == 366)
    return pixel


def inside_safe(x, y):
    # 已确认保守的 R96、内缩 8 px 的轮廓；它不是厂商标注的半径。
    cx, cy = min(384, max(96, x)), min(384, max(96, y))
    return 8 <= x < 472 and 8 <= y < 472 and (x-cx)**2+(y-cy)**2 <= 88**2


def has_pet(pixel):
    return any(pixel(x, y) != (0, 0, 0) for y in range(190, 360) for x in range(16, 290))


def has_bubble(pixel):
    return any(pixel(x, y) != (0, 0, 0) for y in range(144, 180) for x in range(24, 280))


with tempfile.TemporaryDirectory(prefix='rg-metro-') as temporary:
    directory = destination or Path(temporary)
    directory.mkdir(parents=True, exist_ok=True)
    full = render(directory, 'five-visible', fixture, 1200)
    assert full(0, 0) == (0, 0, 0), '屏幕底色必须是纯黑'
    assert full(111, 145) == (0, 0, 0), '气泡容器必须透明，不得出现白色底板'
    assert full(149, 182) == (0, 0, 0), '透明气泡不得残留白色尾巴'
    colors = [(191, 41, 66), (231, 180, 43), (8, 110, 181), (8, 126, 86), (65, 71, 78)]
    for row, color in enumerate(colors):
        assert full(300, 151 + row * 58) == color, f'第 {row + 1} 行不是固定 Metro 色块'
    for y, percentage in [(98, 47), (134, 22)]:
        filled = sum(full(x, y) == (53, 185, 130) for x in range(24, 456))
        assert abs(filled - round(432 * percentage / 100)) <= 1, f'{percentage}% 进度条长度错误：{filled}'
    for y in range(480):
        for x in range(480):
            if full(x, y) == (245, 246, 248) or full(x, y) == (255, 255, 255):
                assert inside_safe(x, y), f'文字/气泡被圆角切到：{x},{y}'

    fading = render(directory, 'five-fading', fixture, 250)
    hidden = render(directory, 'five-hidden', fixture, 500)
    restored = render(directory, 'five-restored', fixture, 1040)
    assert 0 < sum(fading(300, 151)) < sum(colors[0]), '焦点色块必须柔和淡出'
    assert all(hidden(x, y) == (0, 0, 0) for y in range(148, 200) for x in range(297, 464)), \
        '焦点色块和文字必须一起完全消失'
    assert restored(300, 151) == colors[0], '焦点色块必须重新显示'
    for row in range(1, 5):
        assert hidden(300, 151 + row * 58) == colors[row], '其他色块不能随焦点闪烁'

    returning = render(directory, 'five-returning', fixture, 840)
    assert 0 < sum(returning(300, 151)) < sum(colors[0]), '先闪烁时必须保留柔和淡入'
    held = render(directory, 'five-hold-after-blink', fixture, 2840)
    assert restored(300, 151) == held(300, 151) == colors[0], '闪烁完成后常亮停留至本周期结束'
    before_switch = render(directory, 'hold-before-switch', fixture, 5500)
    assert before_switch.hero == 'red-green' and before_switch(300, 151) == colors[0], \
        '第二次闪烁后的停留期结束前不能切换主角'

    # 定时器每 40 ms 实际运行一帧；480/520 ms 的 1/255 低于 LVGL 可见阈值。
    boundary = render(directory, 'first-hidden-boundary', fixture, 540)
    assert boundary(300, 151) == (0, 0, 0), '透明度必须使用调度轮次起点，不能延后到首次绘制'
    second_hidden = render(directory, 'second-hidden', fixture, 3500)
    assert second_hidden.hero == 'red-green' and second_hidden(300, 151) == (0, 0, 0), \
        '第二次消失前不能切换主角'
    assert has_pet(second_hidden) and has_bubble(second_hidden), '第二次只闪色块，宠物气泡保持显示'
    switched = render(directory, 'next-start', fixture, 6040)
    assert switched.hero == 'red-green', '新轮次先让旧宠物退场，不能立刻换图和名称'
    assert switched(300, 151) == colors[0] and switched(300, 267) == colors[2], \
        '切换帧旧色块恢复常亮，新色块从完整亮度开始淡出'
    next_hidden = render(directory, 'next-hidden', fixture, 6500)
    assert next_hidden.hero == 'red-green' and next_hidden(300, 267) == (0, 0, 0), \
        '下一主角色块必须从切换时刻开始计算闪烁'
    assert not has_pet(next_hidden) and not has_bubble(next_hidden), '宠物、气泡与色块必须同时全隐'
    midpoint = render(directory, 'next-midpoint', fixture, 6540)
    assert midpoint.hero == '工具箱 #1' and not has_pet(midpoint) and not has_bubble(midpoint), \
        '完全隐藏时更换宠物和对应文字'
    visible = render(directory, 'next-visible', fixture, 7040)
    assert visible.hero == '工具箱 #1' and has_pet(visible) and has_bubble(visible), '一秒时宠物气泡恢复'
    second = render(directory, 'next-second-blink', fixture, 9500)
    assert second(300, 267) == (0, 0, 0) and has_pet(second) and has_bubble(second), \
        '下一轮的第二次也只闪色块'

    initial = json.loads(json.dumps(fixture))
    for session in initial['sessions']:
        session['state'] = 'running'
    updated = json.loads(json.dumps(initial))
    updated['sessions'][1]['state'] = 'awaiting'
    timeline = [initial] + [updated] * 29
    before = render(directory, 'priority-waits', timeline, 3500)
    assert before.hero == 'red-green' and before(300, 151) == (0, 0, 0), \
        '重复快照/新优先状态不能截断或延期第二次闪烁'
    assert before(300, 209) == colors[0], '新状态应立即更新，不等待主角切换'
    after = render(directory, 'priority-next', timeline, 7040)
    assert after.hero == updated['sessions'][1]['name'] and after(300, 209) == colors[0], \
        '待处理会话在完整轮次结束后成为主角并立即开始闪烁'

    for status in ['unknown', 'expired']:
        model = json.loads(json.dumps(fixture))
        for window in model['usage'][0]['windows']:
            window['used_percentage'] = None
            window['status'] = status
        pixel = render(directory, status, model, 1000)
        edge = [pixel(x, 95) for x in range(24, 456)]
        assert (115, 115, 115) in edge and (0, 0, 0) in edge, '未知/过期应显示虚线空轨道'
        assert all(pixel(x, 98) != (53, 185, 130) for x in range(24, 456)), '未知用量不得填充进度'

    model = json.loads(json.dumps(fixture))
    model['sessions'] = model['sessions'][:1]
    one = render(directory, 'one', model, 1000)
    assert one(300, 209) == (0, 0, 0), '少于五个会话时不得保留旧色块'
    model = json.loads(json.dumps(initial))
    sixth = dict(model['sessions'][0], id='s5', slot=5, name='其他页', state='awaiting')
    model['sessions'].append(sixth)
    second_page = render(directory, 'six-page-two', model, 1200)
    assert second_page.hero == '其他页' and second_page(300, 151) == colors[0], \
        '第六个主角对应色块必须立即出现在第二页首行'
    assert second_page(300, 209) == (0, 0, 0), '翻页后不得留下前页的色块'
    first_page = render(directory, 'six-back-to-first', model, 6040)
    assert first_page.hero == '其他页' and first_page(300, 209) == colors[1], \
        '轮次开始总览先跟随目标，旧宠物仍使用旧名称退场'
    first_hero = render(directory, 'six-new-hero', model, 6540)
    assert first_hero.hero == 'red-green', '跨页新主角在中点才切换'

    single = dict(fixture, sessions=fixture['sessions'][:1])
    single_later = render(directory, 'one-no-exit', single, 6500)
    assert has_pet(single_later) and has_bubble(single_later), '单只宠物不能每轮退场'
    reordered = dict(fixture, sessions=list(reversed(fixture['sessions'])))
    updates = [fixture] * 31 + [reordered] * 5
    assert render(directory, 'reordered-exit', updates, 6300).hero == 'red-green'
    assert render(directory, 'reordered-enter', updates, 6800).hero == '工具箱 #1'
    deleted = dict(fixture, sessions=fixture['sessions'][1:])
    removed = render(directory, 'outgoing-deleted', [fixture] * 31 + [deleted], 6300)
    assert removed.hero != 'red-green', '删除退场对象后不能继续显示旧名称'
    print('Metro: 布局、圆角、同步进退场、中点换宠、两次/六秒、更新/删除与跨页通过')
