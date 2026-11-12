Feature: Reading what a sub-agent did
  As someone whose agents hand work to sub-agents
  I want to open a sub-agent and read its own transcript
  So I can see what it did without digging through the thread

  Background:
    Given the app is connected to a runner with the demo threads

  @header.subagent @sidebar.subagents
  Scenario: A sub-agent opens in place of its thread, and Back returns to the thread
    When I open the sub-agent "Compare the cache keys" of "Why is the build slow on CI?"
    Then the transcript shows the sub-agent's brief "Compare the cache keys in run-812.log"
    When I go back to the thread
    Then the transcript shows the thread "Why is the build slow on CI?"

  @header.subagent
  Scenario: A sub-agent with no record says so
    Given the runner has no record of the sub-agent "Check the Docker layer cache" of "Why is the build slow on CI?"
    When I open the sub-agent "Check the Docker layer cache" of "Why is the build slow on CI?"
    Then the app says it has no record of that sub-agent
