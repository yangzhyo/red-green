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
    subprocess.run([str(preview), str(source), str(output), str(end)],
                   check=True, stdout=subprocess.DEVNULL)
    header, pixels = output.read_bytes().split(b'\n255\n', 1)
    assert header == b'P6\n480 480'
    return lambda x, y: tuple(pixels[(y * 480 + x) * 3:(y * 480 + x) * 3 + 3])


def inside_safe(x, y):
    # 已确认保守的 R96、内缩 8 px 的轮廓；它不是厂商标注的半径。
    cx, cy = min(384, max(96, x)), min(384, max(96, y))
    return 8 <= x < 472 and 8 <= y < 472 and (x-cx)**2+(y-cy)**2 <= 88**2


with tempfile.TemporaryDirectory(prefix='rg-metro-') as temporary:
    directory = destination or Path(temporary)
    directory.mkdir(parents=True, exist_ok=True)
    full = render(directory, 'five-visible', fixture, 1000)
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

    fading = render(directory, 'five-fading', fixture, 2250)
    hidden = render(directory, 'five-hidden', fixture, 2500)
    restored = render(directory, 'five-restored', fixture, 3200)
    assert 0 < sum(fading(300, 151)) < sum(colors[0]), '焦点色块必须柔和淡出'
    assert all(hidden(x, y) == (0, 0, 0) for y in range(148, 200) for x in range(297, 464)), \
        '焦点色块和文字必须一起完全消失'
    assert restored(300, 151) == colors[0], '焦点色块必须重新显示'
    for row in range(1, 5):
        assert hidden(300, 151 + row * 58) == colors[row], '其他色块不能随焦点闪烁'

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
    model = json.loads(json.dumps(fixture))
    sixth = dict(model['sessions'][0], id='s5', slot=5, name='其他页', state='running')
    model['sessions'].append(sixth)
    second_page = render(directory, 'six-page-two', model, 8500)
    assert second_page(300, 151) == colors[1], '第六个会话应出现在第二页首行'
    assert second_page(300, 209) == (0, 0, 0), '翻页后不得留下前页的色块'
    print('Metro: 黑底、五行、进度条、未知/过期、整体闪烁、圆角安全区与分页通过')
