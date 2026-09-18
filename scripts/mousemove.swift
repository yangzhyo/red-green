// 验证用：把光标从 (x0,y0) 分步移到 (x1,y1)，逻辑坐标。用量表的悬停靠 Rust 轮询光标位置，
// 端到端验证时需要真的把光标移过去；分步而非跳跃，接近真实鼠标轨迹。
// 用法：swiftc -O scripts/mousemove.swift -o /tmp/mousemove && /tmp/mousemove x0 y0 x1 y1
import Foundation
import CoreGraphics

func location() -> CGPoint { CGEvent(source: nil)!.location }

let a = CommandLine.arguments
guard a.count == 5, let x0 = Double(a[1]), let y0 = Double(a[2]), let x1 = Double(a[3]), let y1 = Double(a[4]) else {
    print("usage: mousemove x0 y0 x1 y1")
    exit(2)
}
print("before: \(location())")
let steps = 12
for i in 0...steps {
    let t = Double(i) / Double(steps)
    let p = CGPoint(x: x0 + (x1 - x0) * t, y: y0 + (y1 - y0) * t)
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(40_000)
}
usleep(300_000)
print("after: \(location())")
