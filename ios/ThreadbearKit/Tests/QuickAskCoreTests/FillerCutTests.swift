import Foundation
import Testing
@testable import QuickAskCore

struct FillerCutTests {
    private let filler = "That's not in the archive, so I'm checking the web."
    private let web = "Perth has a Mediterranean climate, with warm, dry summers and mild, wet winters."
    private let cutAtWord: [FillerCut.Step] = [.stop(.word), .waitAtMost(FillerCut.wordStopWait)]

    @Test func archiveAnswerNeverCuts() {
        var plan = FillerCut()
        #expect(plan.sentence("Yesterday you worked on Pebble.", at: 0) == [.speak(id: 0, text: "Yesterday you worked on Pebble.")])
        plan.started(0, at: 0.1)
        #expect(plan.sentence("Then Recipe Box.", at: 0.5) == [.speak(id: 1, text: "Then Recipe Box.")])
        #expect(plan.sentence("And Lantern.", at: 0.6) == [.speak(id: 2, text: "And Lantern.")])
        #expect(plan.finished(0, quiet: false, at: 2) == [])
        #expect(plan.finished(1, quiet: false, at: 3) == [])
        #expect(!plan.isIdle)
        #expect(plan.finished(2, quiet: true, at: 4) == [])
        #expect(plan.isIdle)
        #expect(plan.cut == nil)
    }

    @Test func webAnswerReadyMidFillerCutsAtAWordThenSaysItInOrder() {
        var plan = FillerCut()
        #expect(plan.sentence(filler, at: 1) == [.speak(id: 0, text: filler)])
        plan.started(0, at: 1.25)
        plan.stageBegan()
        #expect(plan.sentence(web, at: 2.5) == cutAtWord)
        #expect(plan.cut?.spokenFor == 1.25)
        // More of the answer while the filler finishes its word: held, in order.
        #expect(plan.sentence("Summers are hot.", at: 2.6) == [])
        #expect(!plan.isIdle)
        #expect(plan.isCutting)
        // Stopping at a word is reported as a finish.
        #expect(plan.finished(0, quiet: true, at: 2.75) == [
            .stopped, .speak(id: 1, text: web), .speak(id: 2, text: "Summers are hot."),
        ])
        #expect(plan.cut?.stoppedAt == 2.75)
        #expect(plan.cut?.released == 2)
        #expect(plan.cut?.timedOut == false)
        // After that, the rest goes straight on.
        #expect(plan.sentence("Winters are wet.", at: 2.8) == [.speak(id: 3, text: "Winters are wet.")])
        for id in 1...3 { _ = plan.finished(id, quiet: id == 3, at: 3) }
        #expect(plan.isIdle)
    }

    @Test func webAnswerAfterTheFillerFinishedChangesNothing() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.started(0, at: 1.1)
        plan.stageBegan()
        #expect(plan.finished(0, quiet: true, at: 4) == [])
        #expect(plan.isIdle)
        #expect(plan.sentence(web, at: 4.5) == [.speak(id: 1, text: web)])
        #expect(plan.cut == nil)
    }

    /// Not dropped outright: stopping straight away and then speaking broke the next sentence in
    /// the simulator when the voice wasn't warm, so it's stopped at the end of its first word.
    @Test func fillerNotYetStartedStopsAfterItsFirstWord() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.stageBegan()
        #expect(plan.sentence(web, at: 1.01) == cutAtWord)
        #expect(plan.cut?.spokenFor == nil)
        #expect(!plan.isIdle)
        // The synthesizer starts it, says "That's" and reports it finished.
        plan.started(0, at: 1.02)
        #expect(plan.finished(0, quiet: true, at: 1.15) == [.stopped, .speak(id: 1, text: web)])
    }

    @Test func refusalThenFillerCutsWhicheverIsSpeaking() {
        let refusal = "I couldn't find that in the archive."
        let then = "So I'm checking the web."
        // The refusal's been said, the filler after it is speaking.
        var plan = FillerCut()
        _ = plan.sentence(refusal, at: 1)
        _ = plan.sentence(then, at: 1)
        plan.started(0, at: 1.25)
        _ = plan.finished(0, quiet: false, at: 3)
        plan.started(1, at: 3)
        plan.stageBegan()
        #expect(plan.sentence(web, at: 3.5) == cutAtWord)
        // Counted from the start of the refusal.
        #expect(plan.cut?.spokenFor == 2.25)
        #expect(plan.finished(1, quiet: true, at: 3.75) == [.stopped, .speak(id: 2, text: web)])

        // The refusal is still speaking and the filler's queued behind it: both go, at the
        // refusal's next word.
        var early = FillerCut()
        _ = early.sentence(refusal, at: 1)
        _ = early.sentence(then, at: 1)
        early.started(0, at: 1.25)
        early.stageBegan()
        #expect(early.sentence(web, at: 1.5) == cutAtWord)
        #expect(early.finished(0, quiet: true, at: 1.75) == [.stopped, .speak(id: 2, text: web)])
        // The one queued behind is never reported on, and isn't waited for: once the answer's
        // been said, that's it.
        #expect(early.finished(2, quiet: true, at: 5) == [])
        #expect(early.isIdle)
    }

    @Test func fillerSentenceStartingJustAsTheCutCameIsWaitedForBriefly() {
        var plan = FillerCut()
        _ = plan.sentence("I couldn't find that in the archive.", at: 1)
        _ = plan.sentence("So I'm checking the web.", at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        _ = plan.sentence(web, at: 2.5)
        // The first had really finished and the second had started, but neither had been reported.
        #expect(plan.finished(0, quiet: false, at: 2.5) == [.waitAtMost(FillerCut.nextSentenceWait)])
        plan.started(1, at: 2.5)
        #expect(plan.finished(1, quiet: true, at: 2.625) == [.stopped, .speak(id: 2, text: web)])
        #expect(plan.cut?.stillSpeaking == true)
        #expect(plan.cut?.timedOut == false)
    }

    @Test func stopThatNeverReportsIsForcedThenSettlesBeforeTheAnswer() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        _ = plan.sentence(web, at: 2)
        _ = plan.sentence("Summers are hot.", at: 2.1)
        #expect(plan.waitRanOut(at: 3.2) == [.stop(.immediate), .waitAtMost(FillerCut.settleWait)])
        #expect(plan.cut?.timedOut == true)
        #expect(plan.isCutting)
        // A report now doesn't hurry it: the answer's not queued on a stop that's still going.
        #expect(plan.finished(0, quiet: true, at: 3.21) == [])
        #expect(plan.waitRanOut(at: 3.5) == [
            .stopped, .speak(id: 1, text: web), .speak(id: 2, text: "Summers are hot."),
        ])
        // Nothing's released twice.
        #expect(plan.waitRanOut(at: 3.6) == [])
        #expect(!plan.isIdle)
    }

    @Test func secondFillerThatNeverReportsIsForcedToo() {
        var plan = FillerCut()
        _ = plan.sentence("I couldn't find that in the archive.", at: 1)
        _ = plan.sentence("So I'm checking the web.", at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        _ = plan.sentence(web, at: 2.5)
        _ = plan.finished(0, quiet: false, at: 2.5)
        #expect(plan.waitRanOut(at: 2.8) == [.stop(.immediate), .waitAtMost(FillerCut.settleWait)])
        #expect(plan.waitRanOut(at: 3.1) == [.stopped, .speak(id: 2, text: web)])
        #expect(plan.cut?.stillSpeaking == true)
        #expect(plan.cut?.timedOut == true)
    }

    /// A quick web answer with the deep stage skipped can arrive whole, done and all, while the
    /// filler's still being said. Nothing may look finished until the last of it has been said.
    @Test func wholeBodyArrivingMidFillerIsHeldThenSaid() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        _ = plan.sentence(web, at: 1.5)
        _ = plan.sentence("Summers are hot.", at: 1.5)
        // The rest the splitter hands over at done, with no full stop.
        _ = plan.sentence("Winters are wet", at: 1.5)
        #expect(!plan.isIdle)
        #expect(plan.finished(0, quiet: true, at: 1.75) == [
            .stopped, .speak(id: 1, text: web), .speak(id: 2, text: "Summers are hot."), .speak(id: 3, text: "Winters are wet"),
        ])
        #expect(!plan.isIdle)
        _ = plan.finished(1, quiet: false, at: 4)
        _ = plan.finished(2, quiet: false, at: 5)
        #expect(!plan.isIdle)
        _ = plan.finished(3, quiet: true, at: 6)
        #expect(plan.isIdle)
    }

    @Test func deepStageNeverCutsTheQuickAnswerOrItsLeadIn() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        // The web answer cuts the filler short...
        _ = plan.sentence(web, at: 1.5)
        _ = plan.sentence("I'll dig a bit deeper.", at: 1.5)
        #expect(plan.finished(0, quiet: true, at: 1.75) == [
            .stopped, .speak(id: 1, text: web), .speak(id: 2, text: "I'll dig a bit deeper."),
        ])
        plan.started(1, at: 1.75)
        // ...and a deeper answer arriving while the quick one's still being said just queues.
        plan.stageBegan()
        #expect(plan.sentence("The southwest gets most of its rain in winter.", at: 2) ==
            [.speak(id: 3, text: "The southwest gets most of its rain in winter.")])
    }

    @Test func deepStageAfterAnUncutWebAnswerDoesntCutEither() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        _ = plan.finished(0, quiet: true, at: 4)
        _ = plan.sentence(web, at: 4.5)
        _ = plan.sentence("I'll dig a bit deeper.", at: 4.5)
        plan.started(1, at: 4.5)
        plan.stageBegan()
        #expect(plan.sentence("Summer sea breezes help.", at: 5) == [.speak(id: 3, text: "Summer sea breezes help.")])
        #expect(plan.cut == nil)
    }

    @Test func digDeeperFollowUpCutsItsOwnFiller() {
        var plan = FillerCut()
        #expect(plan.sentence("I'll dig a bit deeper.", at: 1) == [.speak(id: 0, text: "I'll dig a bit deeper.")])
        plan.started(0, at: 1)
        plan.stageBegan()
        #expect(plan.sentence("The southwest gets most of its rain in winter.", at: 1.5) == cutAtWord)
        #expect(plan.finished(0, quiet: true, at: 1.75) == [
            .stopped, .speak(id: 1, text: "The southwest gets most of its rain in winter."),
        ])
    }

    @Test func stageWithNothingSaidBeforeItDoesntCut() {
        var plan = FillerCut()
        plan.stageBegan()
        #expect(plan.sentence(web, at: 1) == [.speak(id: 0, text: web)])
        #expect(plan.cut == nil)
    }

    @Test func stoppingDuringACutStopsEverything() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        _ = plan.sentence(web, at: 1.5)
        _ = plan.sentence("Summers are hot.", at: 1.6)
        plan.reset()
        #expect(plan.isIdle)
        #expect(!plan.isCutting)
        #expect(plan.cut == nil)
        // The stopped filler reporting late, and the wait running out, release nothing.
        #expect(plan.finished(0, quiet: true, at: 1.75) == [])
        #expect(plan.waitRanOut(at: 2.7) == [])
        #expect(plan.isIdle)
        // The next answer starts afresh, with numbers that weren't used before.
        #expect(plan.sentence("Hello.", at: 3) == [.speak(id: 3, text: "Hello.")])
    }

    @Test func stoppingJustAfterACutLandedStopsTheAnswerToo() {
        var plan = FillerCut()
        _ = plan.sentence(filler, at: 1)
        plan.started(0, at: 1)
        plan.stageBegan()
        _ = plan.sentence(web, at: 1.5)
        _ = plan.finished(0, quiet: true, at: 1.75)
        #expect(!plan.isIdle)
        plan.reset()
        #expect(plan.isIdle)
        #expect(plan.finished(1, quiet: true, at: 2) == [])
        #expect(plan.isIdle)
    }

    /// AskModel's path: the NDJSON stream through the sentence splitter, which is flushed at each
    /// stage line, into the plan. Only what came before the first stage is cut.
    @Test func decodedStreamCutsOnlyWhatCameBeforeTheFirstStage() {
        var decoder = ServerAnswerDecoder(contentType: "application/x-ndjson")
        var splitter = SentenceSplitter()
        var plan = FillerCut()
        var steps: [FillerCut.Step] = []
        let lines = [
            #"{"type":"text","text":"I couldn't find that in the archive. So I'm checking "}"#,
            #"{"type":"text","text":"the web. "}"#,
            #"{"type":"stage","stage":"web"}"#,
            #"{"type":"text","text":"Perth is warm. "}"#,
            #"{"type":"text","text":"I'll dig a bit deeper. "}"#,
            #"{"type":"stage","stage":"deep"}"#,
            #"{"type":"text","text":"It rains in winter."}"#,
            #"{"type":"done","deep":"answered"}"#,
        ].joined(separator: "\n") + "\n"
        var time = 1.0
        for event in lines.utf8.compactMap({ decoder.push(byte: $0) }) {
            time += 0.125
            switch event {
            case .text(let text):
                for sentence in splitter.append(text) { steps += plan.sentence(sentence, at: time) }
            case .stage:
                if let rest = splitter.finish() { steps += plan.sentence(rest, at: time) }
                plan.stageBegan()
            case .done:
                if let rest = splitter.finish() { steps += plan.sentence(rest, at: time) }
            default: break
            }
            if time == 1.125 { plan.started(0, at: time) }
        }
        #expect(steps == [
            .speak(id: 0, text: "I couldn't find that in the archive."),
            .speak(id: 1, text: "So I'm checking the web."),
        ] + cutAtWord)
        #expect(plan.finished(0, quiet: true, at: 2) == [
            .stopped,
            .speak(id: 2, text: "Perth is warm."),
            .speak(id: 3, text: "I'll dig a bit deeper."),
            .speak(id: 4, text: "It rains in winter."),
        ])
    }
}
