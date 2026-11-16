import Testing
@testable import QuickAskCore

struct EndOfSpeechTests {
    /// Simulates a mic: `loud` dB while talking, `quiet` dB otherwise, a buffer every 20 ms.
    private func feedLevels(_ detector: inout EndOfSpeechDetector, from start: Double, to end: Double, db: Float) {
        var t = start
        while t < end - 0.0001 {
            detector.audioLevel(db, at: t)
            t += 0.02
        }
    }

    @Test func endsAfterSilenceOnceTheTranscriptSettles() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)        // room noise
        feedLevels(&detector, from: 0.3, to: 1.5, db: -25)      // talking, the last loud buffer at 1.48
        detector.transcriptChanged("What did I", at: 0.9)
        detector.transcriptChanged("What did I do yesterday", at: 1.7) // the recogniser lags the voice
        feedLevels(&detector, from: 1.5, to: 3, db: -62)

        #expect(detector.endOfSpeech(at: 2.15) == nil)          // only 670 ms since the voice stopped
        let end = detector.endOfSpeech(at: 2.2)                 // 720 ms: the setting, timed from the voice
        // It estimates the end from the last loud buffer, not the late transcript
        #expect(end != nil && abs(end! - 1.48) < 0.03)
    }

    @Test func stoppingOnTheWaitsLongerForTheRest() {
        // "Which direction did I take the… Recipe Box design", as asked on the phone on 24 October.
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        feedLevels(&detector, from: 0.3, to: 1.5, db: -25)
        detector.transcriptChanged("Which direction did I take the", at: 1.6)
        feedLevels(&detector, from: 1.5, to: 4, db: -62)

        #expect(detector.endOfSpeech(at: 2.3) == nil)           // the usual 700 ms isn't enough here
        #expect(!EarlySend.isPaused(detector, at: 2.3))         // nor worth sending half of it early
        let end = detector.endOfSpeech(at: 3.0)                 // 1.5 s after the voice stopped
        #expect(end != nil && abs(end! - 1.48) < 0.03)
    }

    @Test func aQuestionEndingOnAPrepositionStillEndsAtTheUsualPause() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        feedLevels(&detector, from: 0.3, to: 1.5, db: -25)
        detector.transcriptChanged("What was I working on", at: 1.6)
        feedLevels(&detector, from: 1.5, to: 3, db: -62)

        #expect(detector.endOfSpeech(at: 2.2) != nil)
    }

    @Test func midSentenceWords() {
        #expect(EndOfSpeechDetector.endsMidSentence(EndOfSpeechDetector.words(in: "What's my")))
        #expect(EndOfSpeechDetector.endsMidSentence(EndOfSpeechDetector.words(in: "Where is, um")))
        #expect(!EndOfSpeechDetector.endsMidSentence(EndOfSpeechDetector.words(in: "Who was I talking to")))
        #expect(!EndOfSpeechDetector.endsMidSentence(EndOfSpeechDetector.words(in: "Where is T3 running?")))
        #expect(!EndOfSpeechDetector.endsMidSentence([]))
    }

    @Test func punctuationRevisionsDontRestartTheWait() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        feedLevels(&detector, from: 0.3, to: 1.5, db: -25)
        detector.transcriptChanged("Where is Paris", at: 1.6)
        feedLevels(&detector, from: 1.5, to: 3, db: -62)
        // After the pause SFSpeechRecognizer goes back and adds punctuation and capitals
        detector.transcriptChanged("Where is Paris?", at: 1.9)
        detector.transcriptChanged("where is Paris?", at: 2.1)

        let end = detector.endOfSpeech(at: 2.2)                 // would have been 2.8 if they'd counted
        #expect(end != nil && abs(end! - 1.48) < 0.03)
        #expect(detector.revisionsIgnored == 2)
        #expect(detector.transcript == "where is Paris?")       // the latest text is still what's sent
        #expect(detector.lastWordChangeAt == 1.6)
        #expect(detector.lastTranscriptChangeAt == 2.1)
    }

    @Test func changedWordsDoRestartTheWait() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        feedLevels(&detector, from: 0.3, to: 1.5, db: -25)
        detector.transcriptChanged("Where is an archive", at: 1.6)
        feedLevels(&detector, from: 1.5, to: 3, db: -62)
        detector.transcriptChanged("Where is my archive", at: 2.0) // re-scored: a different word
        #expect(detector.endOfSpeech(at: 2.3) == nil)           // the words have only been still 300 ms
        #expect(detector.endOfSpeech(at: 2.36) != nil)
        #expect(detector.revisionsIgnored == 0)
    }

    @Test func aRecogniserThatsCatchingUpHoldsItBack() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        feedLevels(&detector, from: 0.3, to: 1.5, db: -25)
        detector.transcriptChanged("What's the name", at: 1.2)
        feedLevels(&detector, from: 1.5, to: 3, db: -62)
        detector.transcriptChanged("What's the name of my son", at: 2.0) // half a second behind
        #expect(detector.endOfSpeech(at: 2.2) == nil)           // quiet long enough, words too fresh
        let end = detector.endOfSpeech(at: 2.36)
        #expect(end != nil && abs(end! - 1.48) < 0.03)
    }

    @Test func keepsListeningWhileYoureStillTalking() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        detector.transcriptChanged("Tell me about", at: 0.5)
        feedLevels(&detector, from: 0.3, to: 1.3, db: -25)      // still talking, no new words yet
        #expect(detector.endOfSpeech(at: 1.3) == nil)           // 800 ms stable, but it's loud
        feedLevels(&detector, from: 1.3, to: 2.1, db: -62)
        #expect(detector.endOfSpeech(at: 1.9) == nil)           // quiet for 620 ms
        #expect(detector.endOfSpeech(at: 2.0) != nil)           // quiet for 720 ms
    }

    @Test func unrecognisedSoundAfterTheWordsStillGetsTheWholePause() {
        // The 364 ms case: the words had settled, but the level stayed up (a word the recogniser
        // didn't catch, or a noise), and it used to send 350 ms after that went quiet.
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        detector.transcriptChanged("Where does T3 code live", at: 0.9)
        feedLevels(&detector, from: 0.3, to: 1.8, db: -25)
        feedLevels(&detector, from: 1.8, to: 3, db: -62)
        #expect(detector.endOfSpeech(at: 2.2) == nil)
        #expect(detector.endOfSpeech(at: 2.4) == nil)           // the words have been still 1.5 s, but it's only 620 ms quiet
        let end = detector.endOfSpeech(at: 2.5)
        #expect(end != nil && abs(end! - 1.78) < 0.03)
    }

    @Test func aLevelThatMissedTheSpeechLeavesItToTheTranscript() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        feedLevels(&detector, from: 0.3, to: 0.6, db: -25)      // one loud word, then a quiet voice
        feedLevels(&detector, from: 0.6, to: 4, db: -62)
        for (time, words) in [(0.8, "So"), (1.2, "So what"), (1.6, "So what about"), (2.0, "So what about the"),
                              (2.4, "So what about the muse")] {
            detector.transcriptChanged(words, at: time)
        }
        // The last word came 1.8 s after the last loud buffer, so the level can't be trusted
        #expect(detector.endOfSpeech(at: 3.05) == nil)
        #expect(detector.endOfSpeech(at: 3.11) == 2.4)          // 700 ms after the last word
    }

    @Test func steadyNoiseDoesntCountAsSpeech() {
        var detector = EndOfSpeechDetector()
        // A noisy café: -38 dB all the time, speech at -20
        feedLevels(&detector, from: 0, to: 0.5, db: -38)
        feedLevels(&detector, from: 0.5, to: 1.5, db: -20)
        detector.transcriptChanged("Where was I up to", at: 1.6)
        feedLevels(&detector, from: 1.5, to: 3, db: -38)
        #expect(detector.noiseFloorDB > -45)
        #expect(!detector.isSpeech(-38))
        #expect(detector.isSpeech(-20))
        #expect(detector.endOfSpeech(at: 2.15) == nil)
        #expect(detector.endOfSpeech(at: 2.2) != nil)
    }

    @Test func noisyLevelStillEndsOnTheTranscriptAlone() {
        var detector = EndOfSpeechDetector()
        detector.transcriptChanged("What's next", at: 0.2)
        // Something loud and changing keeps the level "speech-like" but adds no words
        var t = 0.0
        while t < 2 {
            detector.audioLevel(t.truncatingRemainder(dividingBy: 0.2) < 0.1 ? -70 : -15, at: t)
            t += 0.02
        }
        #expect(detector.endOfSpeech(at: 1.0) == nil)
        #expect(detector.endOfSpeech(at: 1.6) == 0.2)           // silence × 2 = 1.4 s after the change
    }

    @Test func earlySendsShortPauseOnlyWaitsBrieflyForTheWords() {
        var detector = EndOfSpeechDetector()
        feedLevels(&detector, from: 0, to: 0.3, db: -62)
        feedLevels(&detector, from: 0.3, to: 1.5, db: -25)
        detector.transcriptChanged("What did I", at: 1.2)
        feedLevels(&detector, from: 1.5, to: 3, db: -62)
        #expect(!EarlySend.isPaused(detector, at: 1.7))                 // quiet for 220 ms
        #expect(EarlySend.isPaused(detector, at: 1.8))                  // quiet for 320 ms
        detector.transcriptChanged("What did I decide", at: 1.85)       // a word that was lagging
        #expect(!EarlySend.isPaused(detector, at: 1.95))                // the new words are 100 ms old
        #expect(EarlySend.isPaused(detector, at: 2.06))                 // 210 ms
        #expect(detector.endOfSpeech(at: 2.06) == nil)
    }

    @Test func nothingMeaningfulNeverEnds() {
        var detector = EndOfSpeechDetector()
        detector.transcriptChanged("Um", at: 0.2)
        #expect(detector.endOfSpeech(at: 5) == nil)
        detector.transcriptChanged("", at: 0.3)
        #expect(detector.endOfSpeech(at: 5) == nil)
        #expect(!EarlySend.isPaused(detector, at: 5))
    }

    @Test func silenceIsConfigurable() {
        var detector = EndOfSpeechDetector(config: .init(silence: 1.2))
        detector.transcriptChanged("How long did that take", at: 1)
        #expect(detector.endOfSpeech(at: 2.0) == nil)
        #expect(detector.endOfSpeech(at: 2.25) != nil)
        #expect(detector.config.wordsSettle == 0.35)
        #expect(EndOfSpeechDetector.Config(silence: 0.4).wordsSettle == 0.2)
    }

    @Test func meaningfulTranscripts() {
        #expect(!EndOfSpeechDetector.isMeaningful(""))
        #expect(!EndOfSpeechDetector.isMeaningful("  . "))
        #expect(!EndOfSpeechDetector.isMeaningful("Hi"))
        #expect(!EndOfSpeechDetector.isMeaningful("Um, uh. Hmm"))
        #expect(EndOfSpeechDetector.isMeaningful("Why?"))
        #expect(EndOfSpeechDetector.isMeaningful("Um, what did I do?"))
    }

    @Test func wordsIgnorePunctuationCapitalsAndApostrophes() {
        let words = EndOfSpeechDetector.words(in:)
        #expect(words("What's the name of my son?") == ["whats", "the", "name", "of", "my", "son"])
        #expect(words("What\u{2019}s the T3-code?") == words("whats the t3 code"))
        #expect(words("Where is Paris") == words("Where is Paris?"))
        #expect(words("Where is Paris") != words("Where is Paris now"))
        #expect(words("  ") == [])
    }
}
