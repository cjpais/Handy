#!/bin/bash
#
# Automated release script for ggml.
#
# Note: Sync from llama.cpp should be done separately via PR process
# prior to running this script.
#
# Usage:
#   ./scripts/release.sh prepare [major|minor|patch] [--dry-run]
#   ./scripts/release.sh finalize [--dry-run]
#
# Two-stage release process:
#
# Stage 1 - Prepare:
# $ ./scripts/release.sh prepare minor
# This creates a release candidate branch with version bump and removes -dev suffix.
# The branch should then be manually pushed and a PR created, reviewed, and merged.
#
# Stage 2 - Finalize:
# $ ./scripts/release.sh finalize
# After the RC PR is merged, this reads the current version from CMakeLists.txt,
# creates the release tag, and prepares the next development cycle.
#
# Prepare stage:
# 1. Creates release candidate branch
# 2. Updates version and removes -dev suffix
# 3. Commits the version bump
#
# Finalize stage:
# 1. Reads current release version from CMakeLists.txt
# 2. Creates signed git tag on master
# 3. Adds -dev suffix back for next development cycle
# 4. Creates branch and commit for development version
#

set -e

if [ ! -f "CMakeLists.txt" ] || [ ! -d "scripts" ]; then
    echo "Error: Must be run from ggml root directory"
    exit 1
fi

# Parse command line arguments
COMMAND=""
VERSION_TYPE=""
DRY_RUN=false

# First argument should be the command
if [ $# -eq 0 ]; then
    echo "Error: Missing command"
    echo "Usage: $0 prepare [major|minor|patch] [--dry-run]"
    echo "       $0 finalize [--dry-run]"
    exit 1
fi

COMMAND="$1"
shift

# Parse remaining arguments
for arg in "$@"; do
    case $arg in
        --dry-run)
            DRY_RUN=true
            ;;
        major|minor|patch)
            if [ "$COMMAND" = "prepare" ]; then
                VERSION_TYPE="$arg"
            else
                echo "Error: Version type only valid for 'prepare' command"
                exit 1
            fi
            ;;
        *)
            echo "Error: Unknown argument '$arg'"
            echo "Usage: $0 prepare [major|minor|patch] [--dry-run]"
            echo "       $0 finalize [--dry-run]"
            exit 1
            ;;
    esac
done

# Validate command
if [[ ! "$COMMAND" =~ ^(prepare|finalize)$ ]]; then
    echo "Error: Command must be 'prepare' or 'finalize'"
    echo "Usage: $0 prepare [major|minor|patch] [--dry-run]"
    echo "       $0 finalize [--dry-run]"
    exit 1
fi

# For prepare command, default to patch if no version type specified
if [ "$COMMAND" = "prepare" ]; then
    VERSION_TYPE="${VERSION_TYPE:-patch}"
    if [[ ! "$VERSION_TYPE" =~ ^(major|minor|patch)$ ]]; then
        echo "Error: Version type must be 'major', 'minor', or 'patch'"
        echo "Usage: $0 prepare [major|minor|patch] [--dry-run]"
        exit 1
    fi
fi

# Common validation functions
check_git_status() {
    # Check for uncommitted changes (skip in dry-run)
    if [ "$DRY_RUN" = false ] && ! git diff-index --quiet HEAD --; then
        echo "Error: You have uncommitted changes. Please commit or stash them first."
        exit 1
    fi
}

check_master_branch() {
    # Ensure we're on master branch
    CURRENT_BRANCH=$(git branch --show-current)
    if [ "$CURRENT_BRANCH" != "master" ]; then
        if [ "$DRY_RUN" = true ]; then
            echo "[dry run] Warning: Not on master branch (currently on: $CURRENT_BRANCH). Continuing with dry-run..."
            echo ""
        else
            echo "Error: Must be on master branch. Currently on: $CURRENT_BRANCH"
            exit 1
        fi
    fi
}

check_master_up_to_date() {
    # Check if we have the latest from master (skip in dry-run)
    if [ "$DRY_RUN" = false ]; then
        echo "Checking if local master is up-to-date with remote..."
        git fetch origin master
        LOCAL=$(git rev-parse HEAD)
        REMOTE=$(git rev-parse origin/master)

        if [ "$LOCAL" != "$REMOTE" ]; then
            echo "Error: Your local master branch is not up-to-date with origin/master."
            echo "Please run 'git pull origin master' first."
            exit 1
        fi
        echo "✓ Local master is up-to-date with remote"
        echo ""
    elif [ "$(git branch --show-current)" = "master" ]; then
        echo "[dry run] Warning: Dry-run mode - not checking if master is up-to-date with remote"
        echo ""
    fi
}

prepare_release() {
    if [ "$DRY_RUN" = true ]; then
        echo "[dry-run] Preparing release (no changes will be made)"
    else
        echo "Starting release preparation..."
    fi
    echo ""

    check_git_status
    check_master_branch
    check_master_up_to_date

    # Extract current version from CMakeLists.txt
    echo "Step 1: Reading current version..."
    MAJOR=$(grep "set(GGML_VERSION_MAJOR" CMakeLists.txt | sed 's/.*MAJOR \([0-9]*\).*/\1/')
    MINOR=$(grep "set(GGML_VERSION_MINOR" CMakeLists.txt | sed 's/.*MINOR \([0-9]*\).*/\1/')
    PATCH=$(grep "set(GGML_VERSION_PATCH" CMakeLists.txt | sed 's/.*PATCH \([0-9]*\).*/\1/')

    echo "Current version: $MAJOR.$MINOR.$PATCH"

    # Calculate new version
    case $VERSION_TYPE in
        major)
            NEW_MAJOR=$((MAJOR + 1))
            NEW_MINOR=0
            NEW_PATCH=0
            ;;
        minor)
            NEW_MAJOR=$MAJOR
            NEW_MINOR=$((MINOR + 1))
            NEW_PATCH=0
            ;;
        patch)
            NEW_MAJOR=$MAJOR
            NEW_MINOR=$MINOR
            NEW_PATCH=$((PATCH + 1))
            ;;
    esac

    NEW_VERSION="$NEW_MAJOR.$NEW_MINOR.$NEW_PATCH"
    RC_BRANCH="ggml-rc-v$NEW_VERSION"
    echo "New release version: $NEW_VERSION"
    echo "Release candidate branch: $RC_BRANCH"
    echo ""

    # Create release candidate branch
    echo "Step 2: Creating release candidate branch..."
    if [ "$DRY_RUN" = true ]; then
        echo "  [dry-run] Would create branch: $RC_BRANCH"
    else
        git checkout -b "$RC_BRANCH"
        echo "✓ Created and switched to branch: $RC_BRANCH"
    fi
    echo ""

    # Update CMakeLists.txt for release
    echo "Step 3: Updating version in CMakeLists.txt..."
    if [ "$DRY_RUN" = true ]; then
        echo "  [dry-run] Would update GGML_VERSION_MAJOR to $NEW_MAJOR"
        echo "  [dry-run] Would update GGML_VERSION_MINOR to $NEW_MINOR"
        echo "  [dry-run] Would update GGML_VERSION_PATCH to $NEW_PATCH"
    else
        sed -i '' -e "s/set(GGML_VERSION_MAJOR [0-9]*)/set(GGML_VERSION_MAJOR $NEW_MAJOR)/" CMakeLists.txt
        sed -i '' -e "s/set(GGML_VERSION_MINOR [0-9]*)/set(GGML_VERSION_MINOR $NEW_MINOR)/" CMakeLists.txt
        sed -i '' -e "s/set(GGML_VERSION_PATCH [0-9]*)/set(GGML_VERSION_PATCH $NEW_PATCH)/" CMakeLists.txt
    fi
    echo ""

    # Commit version bump
    echo "Step 4: Committing version bump..."
    if [ "$DRY_RUN" = true ]; then
        echo "  [dry-run] Would commit: 'ggml : bump version to $NEW_VERSION'"
    else
        git add CMakeLists.txt
        git commit -m "ggml : bump version to $NEW_VERSION"
    fi
    echo ""

    echo ""
    if [ "$DRY_RUN" = true ]; then
        echo "[dry-run] Summary (no changes were made):"
        echo "  • Would have created branch: $RC_BRANCH"
        echo "  • Would have updated version to: $NEW_VERSION"
    else
        echo "Release preparation completed!"
        echo "Summary:"
        echo "  • Created branch: $RC_BRANCH"
        echo "  • Updated version to: $NEW_VERSION"
        echo ""
        echo "Next steps:"
        echo "  • Push branch to remote: git push origin $RC_BRANCH"
        echo "  • Create a Pull Request from $RC_BRANCH to master"
        echo "  • After PR is merged, run: ./scripts/release.sh finalize"
    fi
}

finalize_release() {
    if [ "$DRY_RUN" = true ]; then
        echo "[dry-run] Finalizing release (no changes will be made)"
    else
        echo "Starting release finalization..."
    fi
    echo ""

    check_git_status
    check_master_branch
    check_master_up_to_date

    # Read current version from CMakeLists.txt
    echo "Step 1: Reading current release version..."
    MAJOR=$(grep "set(GGML_VERSION_MAJOR" CMakeLists.txt | sed 's/.*MAJOR \([0-9]*\).*/\1/')
    MINOR=$(grep "set(GGML_VERSION_MINOR" CMakeLists.txt | sed 's/.*MINOR \([0-9]*\).*/\1/')
    PATCH=$(grep "set(GGML_VERSION_PATCH" CMakeLists.txt | sed 's/.*PATCH \([0-9]*\).*/\1/')

    RELEASE_VERSION="$MAJOR.$MINOR.$PATCH"
    echo "Release version: $RELEASE_VERSION"
    echo ""

    # Create git tag
    echo "Step 2: Creating signed git tag..."
    if [ "$DRY_RUN" = true ]; then
        echo "  [dry-run] Would create signed tag: v$RELEASE_VERSION with message 'Release version $RELEASE_VERSION'"
    else
        git tag -s "v$RELEASE_VERSION" -m "Release version $RELEASE_VERSION"
        echo "✓ Created signed tag: v$RELEASE_VERSION"
    fi
    echo ""


    echo ""
    if [ "$DRY_RUN" = true ]; then
        echo "[dry-run] Summary (no changes were made):"
        echo "  • Would have created tag: v$RELEASE_VERSION"
    else
        echo "Release finalization completed!"
        echo "Summary:"
        echo "  • Created signed tag: v$RELEASE_VERSION"
        echo ""
        echo "Next steps:"
        echo "  • Push tag to remote: git push origin v$RELEASE_VERSION"
        echo "  • The release is now complete!"
    fi
}

# Execute the appropriate command
case $COMMAND in
    prepare)
        prepare_release
        ;;
    finalize)
        finalize_release
        ;;
esac
