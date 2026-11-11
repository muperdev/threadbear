import SwiftUI
import ThreadbearCore

/// The open thread's login is at its limit, or its last turn stopped on
/// it: a box at the top of the composer, over a line like the card of
/// asks, saying what happened, with Switch to the login with the most room
/// and whether it carries on by itself once the limit resets (on unless
/// he's turned it off for this thread). Mike asked for it here on 10 Nov,
/// in place of a bar across the top of the thread that read as an error
/// and offered only the switch.
///
/// Here rather than in the transcript's error row, whose drawing is cached
/// and knows nothing of accounts, so the name and the room it offers stay
/// current as usage changes.
struct LimitBox: View {
    @Environment(AppModel.self) private var app
    let thread: ThreadSummary
    var padding: CGFloat

    var body: some View {
        if let advice = app.limitAdvice(for: thread) {
            // Waiting to carry on reads as a quiet clock, not the red gauge.
            let waiting = advice.carriesOnAt != nil && thread.carryOnAtReset
            let text = advice.text + (waiting ? advice.carriesOnAt.map { " Carries on by itself " + Self.when($0, now: app.now) + "." } ?? "" : "")
            VStack(alignment: .leading, spacing: zoomed(8)) {
                HStack(alignment: .firstTextBaseline, spacing: zoomed(8)) {
                    Image(systemName: waiting ? "clock" : "gauge.with.dots.needle.100percent")
                        .foregroundStyle(waiting ? Color.secondary : Color.red)
                    Text(text)
                        .fixedSize(horizontal: false, vertical: true)
                        .tip(text)
                }
                if advice.target != nil || advice.stopped {
                    HStack(spacing: zoomed(10)) {
                        if let target = advice.target { switchButton(target, pool: advice.pool) }
                        if advice.stopped { carryOnMenu(advice) }
                        Spacer(minLength: 0)
                    }
                }
            }
            .font(.system(size: TypeScale.label))
            .padding(.horizontal, padding)
            .padding(.top, zoomed(10))
            .padding(.bottom, zoomed(10))
            Rectangle()
                .fill(Color(nsColor: .separatorColor))
                .frame(height: 0.5)
        }
    }

    private func switchButton(_ target: Account, pool: Bool) -> some View {
        let title = AccountUsageText.switchTitle(target, accounts: app.accounts)
        // A pool thread's switch pins it to a login outside the pool.
        return Button(pool ? title.replacingOccurrences(of: "Switch to", with: "Pin to") : title) {
            app.switchAccount(thread.id, to: target.id)
        }
        .busy(app.isBusy(BusyKey.moving(thread.id)))
        .fixedSize()
        .tip("Carry on this thread on that login now, from where it is")
    }

    private func carryOnMenu(_ advice: AppModel.LimitAdvice) -> some View {
        let on = thread.carryOnAtReset
        let when = advice.carriesOnAt.map { Self.when($0, now: app.now) } ?? "once its limit resets"
        return Picker(selection: Binding(get: { on }, set: { app.setCarryOnAtReset(thread.id, $0) })) {
            Text("Yes").tag(true)
            Text("No").tag(false)
        } label: {
            Text("Carry on by itself").foregroundStyle(.secondary)
        }
        .pickerStyle(.menu)
        .fixedSize()
        .tip(on ? "It carries on from where it stopped \(when). No to leave it until you send a message"
                : "It waits for you to send a message. Yes to carry on from where it stopped once there's room")
    }

    /// "at 4:41 pm", "at tomorrow 5:00 am" reads badly, so a day goes
    /// without the "at"; a time that's passed is "any moment now".
    static func when(_ date: Date, now: Date) -> String {
        let time = Pools.waitingTime(date, now: now)
        return time.first?.isNumber == true ? "at " + time : time
    }
}
