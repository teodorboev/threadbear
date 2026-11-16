import Testing
@testable import QuickAskCore

struct SentenceSplitterTests {
    /// Feeds the chunks in order and collects everything spoken, including the end.
    private func split(_ chunks: [String], eager: Int? = nil) -> [String] {
        var splitter = SentenceSplitter(eagerFirstClauseLength: eager)
        var out: [String] = []
        for chunk in chunks { out += splitter.append(chunk) }
        if let rest = splitter.finish() { out.append(rest) }
        return out
    }

    @Test func wholeTextInOneChunk() {
        #expect(split(["Hello there. How are you? I'm fine!"]) == ["Hello there.", "How are you?", "I'm fine!"])
    }

    @Test func chunkBoundaryMidWord() {
        #expect(split(["Yesterday you wor", "ked on Peb", "ble. Then Re", "cipe Box."]) ==
            ["Yesterday you worked on Pebble.", "Then Recipe Box."])
    }

    @Test func chunkBoundaryRightAfterTheFullStop() {
        // "on Pebble." could still become "on Pebble.com", so it waits for the next character
        var splitter = SentenceSplitter(eagerFirstClauseLength: nil)
        #expect(splitter.append("You worked on Pebble.") == [])
        #expect(splitter.append(" Then") == ["You worked on Pebble."])
        #expect(splitter.finish() == "Then")
    }

    @Test func oneCharacterAtATime() {
        let text = "First one. Second one? Third."
        #expect(split(text.map(String.init)) == ["First one.", "Second one?", "Third."])
    }

    @Test func emitsAsSoonAsASentenceIsComplete() {
        var splitter = SentenceSplitter(eagerFirstClauseLength: nil)
        #expect(splitter.append("It was on Tues") == [])
        #expect(splitter.append("day. You") == ["It was on Tuesday."])
        #expect(splitter.append(" also fixed the build.") == [])
        #expect(splitter.append("\n") == ["You also fixed the build."])
    }

    @Test func decimalsAndDomainsDontSplit() {
        #expect(split(["It took 3.5 hours on convex.dev. Then you stopped."]) ==
            ["It took 3.5 hours on convex.dev.", "Then you stopped."])
    }

    @Test func abbreviationsAndInitialsDontSplit() {
        #expect(split(["You met Dr. Smith, e.g. about J. R. R. Tolkien. Fun."]) ==
            ["You met Dr. Smith, e.g. about J. R. R. Tolkien.", "Fun."])
    }

    @Test func closingQuotesStayWithTheSentence() {
        #expect(split(["He said \"ship it.\" Then you did."]) == ["He said \"ship it.\"", "Then you did."])
    }

    @Test func lineBreaksEndASentence() {
        #expect(split(["First line without a stop\nSecond line"]) == ["First line without a stop", "Second line"])
    }

    @Test func listNumbersDontSplit() {
        #expect(split(["1. Fix the build. 2. Ship it."]) == ["1. Fix the build.", "2. Ship it."])
    }

    @Test func ellipsisAndRepeatedPunctuation() {
        #expect(split(["Well... maybe?! Yes."]) == ["Well...", "maybe?!", "Yes."])
    }

    @Test func emptyAndWhitespaceOnly() {
        #expect(split([]) == [])
        #expect(split(["   ", "\n\n"]) == [])
    }

    @Test func eagerFirstClauseCutsALongOpeningAtAComma() {
        var splitter = SentenceSplitter(eagerFirstClauseLength: 20)
        #expect(splitter.append("Last Tuesday you worked on the pricing page,") == [])
        #expect(splitter.append(" and then you") == ["Last Tuesday you worked on the pricing page,"])
        // Only the first piece is cut early; after that it waits for whole sentences
        #expect(splitter.append(" fixed the toggle, which was broken.") == [])
        #expect(splitter.append(" Done.") == ["and then you fixed the toggle, which was broken."])
        #expect(splitter.finish() == "Done.")
    }

    @Test func eagerFirstClauseLeavesShortOpeningsAlone() {
        #expect(split(["Yes, you did. It was Tuesday."], eager: 40) == ["Yes, you did.", "It was Tuesday."])
    }
}

struct UTF8StreamDecoderTests {
    @Test func multiByteCharacterSplitAcrossChunks() {
        let bytes = Array("Café — 👍 done".utf8)
        var decoder = UTF8StreamDecoder()
        var text = ""
        for byte in bytes { text += decoder.decode(byte: byte) ?? "" }
        text += decoder.finish()
        #expect(text == "Café — 👍 done")
    }

    @Test func holdsBackAnIncompleteTail() {
        var decoder = UTF8StreamDecoder()
        let thumbs = Array("👍".utf8)
        #expect(decoder.decode([UInt8(ascii: "a")] + thumbs.prefix(2)) == "a")
        #expect(decoder.decode(thumbs.suffix(2)) == "👍")
    }

    @Test func truncatedStreamEndsWithReplacementCharacter() {
        var decoder = UTF8StreamDecoder()
        #expect(decoder.decode(Array("é".utf8).prefix(1)) == "")
        #expect(decoder.finish() == "\u{FFFD}")
    }
}
