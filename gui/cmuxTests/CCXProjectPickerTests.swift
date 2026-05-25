import Foundation
import XCTest

#if DEBUG
@testable import cmux_DEV
#else
@testable import cmux
#endif

@MainActor
final class CCXProjectPickerTests: XCTestCase {
    func testLaunchArgumentsDoNotRequestCCXForOrdinaryLaunch() {
        let args = CCXLaunchArguments.parse(["cmux"])

        XCTAssertFalse(args.isCCXLaunch)
        XCTAssertNil(args.projectId)
    }

    func testLaunchArgumentsRequestPickerWithCCXFlag() {
        let args = CCXLaunchArguments.parse(["cmux", "--ccx"])

        XCTAssertTrue(args.isCCXLaunch)
        XCTAssertNil(args.projectId)
    }

    func testLaunchArgumentsRequestDashboardWithProjectId() {
        let args = CCXLaunchArguments.parse(["cmux", "--project-id", "p_1"])

        XCTAssertTrue(args.isCCXLaunch)
        XCTAssertEqual(args.projectId, "p_1")
    }

    func testPanelWithoutProjectUsesPickerMode() {
        let panel = CCXDashboardPanel(projectId: nil, ccxHome: temporaryHome())

        XCTAssertNil(panel.projectStore)
        XCTAssertEqual(panel.displayTitle, "CCX Projects")
    }

    func testPanelWithProjectUsesDashboardMode() {
        let panel = CCXDashboardPanel(projectId: "p_1", ccxHome: temporaryHome())

        XCTAssertNotNil(panel.projectStore)
        XCTAssertEqual(panel.displayTitle, "CCX")
    }

    func testPanelUsesInjectedProjectsStore() {
        let store = CCXProjectsStore(ccxHome: temporaryHome())
        let panel = CCXDashboardPanel(projectId: nil, projectsStore: store)

        XCTAssertTrue(panel.projectsStore === store)
    }

    func testPickerRowModelUsesSummaryFields() {
        let summary = CCXProjectSummary(
            projectId: "p_1",
            displaySlug: "repo",
            canonicalRepo: "/repo",
            taskSourceFile: "/repo/z/tasks.md",
            createdAt: "2026-05-25T00:00:00Z"
        )

        let model = CCXProjectPickerRowModel(summary: summary)

        XCTAssertEqual(model.id, "p_1")
        XCTAssertEqual(model.title, "repo")
        XCTAssertEqual(model.subtitle, "/repo")
        XCTAssertEqual(model.taskSourceFile, "/repo/z/tasks.md")
    }

    func testPickerRowModelFallsBackToProjectIdForEmptySlug() {
        let summary = CCXProjectSummary(
            projectId: "p_fallback",
            displaySlug: "",
            canonicalRepo: "/repo",
            taskSourceFile: "",
            createdAt: ""
        )

        XCTAssertEqual(CCXProjectPickerRowModel(summary: summary).title, "p_fallback")
    }

    private func temporaryHome() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("CCXProjectPickerTests-\(UUID().uuidString)", isDirectory: true)
    }
}
