#![cfg(test)]

use anyhow::Context;
use futures::{pin_mut, stream::StreamExt};
use matrix_sdk::{Client, RoomListEntry, SlidingSyncBuilder, SlidingSyncList, SlidingSyncMode};
use matrix_sdk_integration_testing::helpers::get_client_for_user;

async fn setup(
    name: String,
    use_sqlite_store: bool,
) -> anyhow::Result<(Client, SlidingSyncBuilder)> {
    let sliding_sync_proxy_url =
        option_env!("SLIDING_SYNC_PROXY_URL").unwrap_or("http://localhost:8338").to_owned();
    let client = get_client_for_user(name, use_sqlite_store).await?;
    let sliding_sync_builder = client
        .sliding_sync()
        .await
        .homeserver(sliding_sync_proxy_url.parse()?)
        .with_common_extensions();
    Ok((client, sliding_sync_builder))
}

#[derive(PartialEq, Eq, Clone, Debug)]
enum RoomListEntryEasy {
    Empty,
    Invalid,
    Filled,
}

impl From<&RoomListEntry> for RoomListEntryEasy {
    fn from(value: &RoomListEntry) -> Self {
        match value {
            RoomListEntry::Empty => RoomListEntryEasy::Empty,
            RoomListEntry::Invalidated(_) => RoomListEntryEasy::Invalid,
            RoomListEntry::Filled(_) => RoomListEntryEasy::Filled,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn it_works_smoke_test() -> anyhow::Result<()> {
    let (_client, sync_builder) = setup("odo".to_owned(), false).await?;
    let sync_proxy = sync_builder
        .add_list(
            SlidingSyncList::builder("foo")
                .sync_mode(SlidingSyncMode::Selective)
                .add_range(0..=10)
                .timeline_limit(0),
        )
        .build()
        .await?;
    let stream = sync_proxy.stream();
    pin_mut!(stream);
    let room_summary =
        stream.next().await.context("No room summary found, loop ended unsuccessfully")?;
    let summary = room_summary?;
    assert_eq!(summary.rooms.len(), 0);
    Ok(())
}

/*
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn modifying_timeline_limit() -> anyhow::Result<()> {
    let (client, sync_builder) = random_setup_with_rooms(1).await?;

    // List one room.
    let room_id = {
        let sync = sync_builder
            .clone()
            .add_list(
                SlidingSyncList::builder("init_list")
                    .sync_mode(SlidingSyncMode::Selective)
                    .add_range(0..=1)
                    .timeline_limit(0)
                    .build(),
            )
            .build()
            .await?;

        // Get the sync stream.
        let stream = sync.stream();
        pin_mut!(stream);

        // Get the list to all rooms to check the list' state.
        let list = sync.list("init_list").context("list `init_list` isn't found")?;
        assert_eq!(list.state(), SlidingSyncState::NotLoaded);

        // Send the request and wait for a response.
        let update_summary =
            stream.next().await.context("No room summary found, loop ended unsuccessfully")??;

        // Check the state has switched to `Live`.
        assert_eq!(list.state(), SlidingSyncState::FullyLoaded);

        // One room has received an update.
        assert_eq!(update_summary.rooms.len(), 1);

        // Let's fetch the room ID then.
        let room_id = update_summary.rooms[0].clone();

        // Let's fetch the room ID from the list too.
        assert_matches!(list.room_list().get(0), Some(RoomListEntry::Filled(same_room_id)) => {
            assert_eq!(same_room_id, &room_id);
        });

        room_id
    };

    // Join a room and send 20 messages.
    {
        // Join the room.
        let room = client.get_joined_room(&room_id).context("Failed to join room `{room_id}`")?;

        // In this room, let's send 20 messages!
        for nth in 0..20 {
            let message = RoomMessageEventContent::text_plain(format!("Message #{nth}"));

            room.send(message, None).await?;
        }

        // Wait on the server to receive all the messages.
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let sync = sync_builder
        .clone()
        .add_list(
            SlidingSyncList::builder("visible_room_list")
                .sync_mode(SlidingSyncMode::Selective)
                .add_range(0..=1)
                .timeline_limit(1)
                .build(),
        )
        .build()
        .await?;

    // Get the sync stream.
    let stream = sync.stream();
    pin_mut!(stream);

    // Get the list.
    let list = sync.list("visible_room_list").context("list `visible_room_list` isn't found")?;

    let mut all_event_ids = Vec::new();

    // Sync to receive a message with a `timeline_limit` set to 1.
    let (room, _timeline, mut timeline_stream) = {
        let mut update_summary;

        loop {
            // Wait for a response.
            update_summary = stream
                .next()
                .await
                .context("No update summary found, loop ended unsuccessfully")??;

            if !update_summary.rooms.is_empty() {
                break;
            }
        }

        // We see that one room has received an update, and it's our room!
        assert_eq!(update_summary.rooms.len(), 1);
        assert_eq!(room_id, update_summary.rooms[0]);

        // OK, now let's read the timeline!
        let room = sync.get_room(&room_id).expect("Failed to get the room");

        // Test the `Timeline`.
        let timeline = room.timeline().await.unwrap();
        let (timeline_items, timeline_stream) = timeline.subscribe().await;

        // First timeline item.
        assert_matches!(timeline_items[0].as_virtual(), Some(_));

        // Second timeline item.
        let latest_remote_event = timeline_items[1].as_event().unwrap();
        all_event_ids.push(latest_remote_event.event_id().unwrap().to_owned());

        // Test the room to see the last event.
        let latest_event = room.latest_event().await.unwrap();
        assert_eq!(
            latest_event.event_id(),
            latest_remote_event.event_id(),
            "Unexpected latest event"
        );
        assert_eq!(latest_event.content().as_message().unwrap().body(), "Message #19");

        (room, timeline, timeline_stream)
    };

    // Sync to receive messages with a `timeline_limit` set to 20.
    {
        list.set_timeline_limit(Some(20));

        let mut update_summary;

        loop {
            // Wait for a response.
            update_summary = stream
                .next()
                .await
                .context("No update summary found, loop ended unsuccessfully")??;

            if !update_summary.rooms.is_empty() {
                break;
            }
        }

        // We see that one room has received an update, and it's our room!
        assert_eq!(update_summary.rooms.len(), 1);
        assert_eq!(room_id, update_summary.rooms[0]);

        // Let's fetch the room ID from the list too.
        assert_matches!(list.room_list().get(0), Some(RoomListEntry::Filled(same_room_id)) => {
            assert_eq!(same_room_id, &room_id);
        });

        // Test the `Timeline`.

        // The first 19th items are `VectorDiff::PushBack`.
        for nth in 0..19 {
            let value = assert_matches!(
                timeline_stream.next().await,
                Some(VectorDiff::PushBack { value }) => value
            );
            let event = value.as_event().unwrap();

            // Check messages arrived in the correct order.
            assert_eq!(
                event.content().as_message().expect("Received event is not a message").body(),
                format!("Message #{nth}"),
            );

            all_event_ids.push(event.event_id().unwrap().to_owned());
        }

        // The 20th item is a `VectorDiff::Remove`, i.e. the first message is removed.
        assert_matches!(timeline_stream.next().await, Some(VectorDiff::Remove { index }) => {
            // Index 0 is for day divider. So our first event is at index 1.
            assert_eq!(index, 1);
        });

        // And now, the initial message is pushed at the bottom, so the 21th item is a
        // `VectorDiff::PushBack`.
        let latest_remote_event = assert_matches!(
            timeline_stream.next().await,
            Some(VectorDiff::PushBack { value }) => value
        );

        let event = latest_remote_event.as_event().unwrap();
        let event_id = event.event_id().unwrap();
        assert_eq!(event.content().as_message().unwrap().body(), "Message #19");
        assert_eq!(event_id, all_event_ids[0]);

        // Test the room to see the last event.
        let latest_event = room.latest_event().await.unwrap();

        assert_eq!(event.content().as_message().unwrap().body(), "Message #19");
        assert_eq!(latest_event.event_id().unwrap(), event_id, "Unexpected latest event");

        // Ensure there is no event ID duplication.
        {
            let mut dedup_event_ids = all_event_ids.clone();
            dedup_event_ids.sort();
            dedup_event_ids.dedup();

            assert_eq!(dedup_event_ids.len(), all_event_ids.len(), "Found duplicated event ID");
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adding_list_later() -> anyhow::Result<()> {
    let list_name_1 = "sliding1";
    let list_name_2 = "sliding2";
    let list_name_3 = "sliding3";

    let (client, sync_proxy_builder) = random_setup_with_rooms(20).await?;
    let build_list = |name| {
        SlidingSyncList::builder(name)
            .sync_mode(SlidingSyncMode::Selective)
            .set_range(0..=10)
            .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
            .build()
    };
    let sync_proxy = sync_proxy_builder
        .add_list(build_list(list_name_1))
        .add_list(build_list(list_name_2))
        .build()
        .await?;
    let list1 = sync_proxy.list(list_name_1).context("but we just added that list!")?;
    let _list2 = sync_proxy.list(list_name_2).context("but we just added that list!")?;

    assert!(sync_proxy.list(list_name_3).is_none());

    let stream = sync_proxy.stream();
    pin_mut!(stream);
    let room_summary =
        stream.next().await.context("No room summary found, loop ended unsuccessfully")?;
    let summary = room_summary?;
    // we only heard about the ones we had asked for
    assert_eq!(summary.lists, [list_name_1, list_name_2]);

    assert!(sync_proxy.add_list(build_list(list_name_3)).is_none());

    // we need to restart the stream after every list listing update
    let stream = sync_proxy.stream();
    pin_mut!(stream);

    let mut saw_update = false;
    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if !summary.lists.is_empty() {
            // only if we saw an update come through
            assert_eq!(summary.lists, [list_name_1, list_name_2, list_name_3]);
            // we didn't update the other lists, so only no 2 should se an update
            saw_update = true;
            break;
        }
    }

    assert!(saw_update, "We didn't see the update come through the pipe");

    // and let's update the order of all lists again
    let room_id = assert_matches!(list1.room_list().get(4), Some(RoomListEntry::Filled(room_id)) => room_id.clone());

    let room = client.get_joined_room(&room_id).context("No joined room {room_id}")?;

    let content = RoomMessageEventContent::text_plain("Hello world");

    room.send(content, None).await?; // this should put our room up to the most recent

    let mut saw_update = false;
    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if !summary.lists.is_empty() {
            // only if we saw an update come through
            assert_eq!(summary.lists, [list_name_1, list_name_2, list_name_3,]);
            // notice that our list 2 is now the last list, but all have seen updates
            saw_update = true;
            break;
        }
    }

    assert!(saw_update, "We didn't see the update come through the pipe");

    Ok(())
}

// index-based lists don't support removing lists. Leaving this test for an API
// update later.
//
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_lists() -> anyhow::Result<()> {
    let list_name_1 = "sliding1";
    let list_name_2 = "sliding2";
    let list_name_3 = "sliding3";

    let (client, sync_proxy_builder) = random_setup_with_rooms(20).await?;
    let build_list = |name| {
        SlidingSyncList::builder(name)
            .sync_mode(SlidingSyncMode::Selective)
            .set_range(0..=10)
            .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
            .build()
    };
    let sync_proxy = sync_proxy_builder
        .add_list(build_list(list_name_1))
        .add_list(build_list(list_name_2))
        .add_list(build_list(list_name_3))
        .build()
        .await?;
    let Some(list1 )= sync_proxy.list(list_name_1) else {
            bail!("but we just added that list!");
        };
    let Some(_list2 )= sync_proxy.list(list_name_2) else {
            bail!("but we just added that list!");
        };

    let Some(_list3 )= sync_proxy.list(list_name_3) else {
            bail!("but we just added that list!");
        };

    let stream = sync_proxy.stream();
    pin_mut!(stream);
    let Some(room_summary ) = stream.next().await else {
            bail!("No room summary found, loop ended unsuccessfully");
        };
    let summary = room_summary?;
    // we only heard about the ones we had asked for
    assert_eq!(summary.lists, [list_name_1, list_name_2, list_name_3]);

    let Some(list_2) = sync_proxy.get_list(&list_name_2.to_owned()) else {
        bail!("Room exists");
    };

    // we need to restart the stream after every list listing update
    let stream = sync_proxy.stream();
    pin_mut!(stream);

    // Let's trigger an update by sending a message to room pos=3, making it move to
    // pos 0

    let room_id = assert_matches!(list1.room_list().get(3), Some(RoomListEntry::Filled(room_id)) => room_id.clone());

    let Some(room) = client.get_joined_room(&room_id) else {
            bail!("No joined room {room_id}");
        };

    let content = RoomMessageEventContent::text_plain("Hello world");

    room.send(content, None).await?; // this should put our room up to the most recent

    let mut saw_update = false;
    for _n in 0..2 {
        let Some(room_summary ) = stream.next().await else {
                bail!("sync has closed unexpectedly");
            };
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if !summary.lists.is_empty() {
            // only if we saw an update come through
            assert_eq!(summary.lists, [list_name_1, list_name_3]);
            saw_update = true;
            break;
        }
    }

    assert!(saw_update, "We didn't see the update come through the pipe");

    assert!(sync_proxy.add_list(list_2).is_none());

    // we need to restart the stream after every list listing update
    let stream = sync_proxy.stream();
    pin_mut!(stream);

    // and let's update the order of all lists again
    let room_id = assert_matches!(list1.room_list().get(4), Some(RoomListEntry::Filled(room_id)) => room_id.clone());

    let Some(room) = client.get_joined_room(&room_id) else {
            bail!("No joined room {room_id}");
        };

    let content = RoomMessageEventContent::text_plain("Hello world");

    room.send(content, None).await?; // this should put our room up to the most recent

    let mut saw_update = false;
    for _n in 0..2 {
        let Some(room_summary ) = stream.next().await else {
                bail!("sync has closed unexpectedly");
            };
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if !summary.lists.is_empty() {
            // only if we saw an update come through
            assert_eq!(summary.lists, [list_name_1, list_name_2, list_name_3]); // all lists are visible again
            saw_update = true;
            break;
        }
    }

    assert!(saw_update, "We didn't see the update come through the pipe");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_goes_live() -> anyhow::Result<()> {
    let (_client, sync_proxy_builder) = random_setup_with_rooms(21).await?;
    let sliding_window_list = SlidingSyncList::builder("sliding")
        .sync_mode(SlidingSyncMode::Selective)
        .set_range(0..=10)
        .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
        .build();

    let full = SlidingSyncList::builder("full")
        .sync_mode(SlidingSyncMode::Growing)
        .full_sync_batch_size(10)
        .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
        .build();
    let sync_proxy =
        sync_proxy_builder.add_list(sliding_window_list).add_list(full).build().await?;

    let list = sync_proxy.list("sliding").context("but we just added that list!")?;
    let full_list = sync_proxy.list("full").context("but we just added that list!")?;
    assert_eq!(list.state(), SlidingSyncState::NotLoaded, "list isn't cold");
    assert_eq!(full_list.state(), SlidingSyncState::NotLoaded, "full isn't cold");

    let stream = sync_proxy.stream();
    pin_mut!(stream);

    // Exactly one poll!
    // Ranges are 0..=9 for selective list, and 0..=9 for growing list.
    let room_summary =
        stream.next().await.context("No room summary found, loop ended unsuccessfully")??;

    // we only heard about the ones we had asked for
    assert_eq!(room_summary.rooms.len(), 11);
    assert_eq!(list.state(), SlidingSyncState::FullyLoaded, "list isn't live");
    assert_eq!(full_list.state(), SlidingSyncState::PartiallyLoaded, "full isn't preloading");

    // Another poll!
    // Ranges are 0..=10 for selective list, and 0..=19 for growing list.
    let _room_summary =
        stream.next().await.context("No room summary found, loop ended unsuccessfully")??;

    let room_list = full_list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        room_list,
        repeat(RoomListEntryEasy::Filled)
            .take(20)
            .chain(once(RoomListEntryEasy::Empty))
            .collect::<Vec<_>>()
    );
    assert_eq!(full_list.state(), SlidingSyncState::PartiallyLoaded, "full isn't preloading");

    // One last poll, and we should get all rooms loaded.
    let _room_summary =
        stream.next().await.context("No room summary found, loop ended unsecessfully")??;

    let room_list = full_list.room_list::<RoomListEntryEasy>();

    assert_eq!(room_list, repeat(RoomListEntryEasy::Filled).take(21).collect::<Vec<_>>());
    assert_eq!(full_list.state(), SlidingSyncState::FullyLoaded, "full isn't fully loaded");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resizing_sliding_window() -> anyhow::Result<()> {
    let (_client, sync_proxy_builder) = random_setup_with_rooms(20).await?;
    let sliding_window_list = SlidingSyncList::builder("sliding")
        .sync_mode(SlidingSyncMode::Selective)
        .set_range(0..=10)
        .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
        .build();
    let sync_proxy = sync_proxy_builder.add_list(sliding_window_list).build().await?;
    let list = sync_proxy.list("sliding").context("but we just added that list!")?;
    let stream = sync_proxy.stream();
    pin_mut!(stream);
    let room_summary =
        stream.next().await.context("No room summary found, loop ended unsuccessfully")?;
    let summary = room_summary?;
    // we only heard about the ones we had asked for
    assert_eq!(summary.rooms.len(), 11);

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Filled)
            .take(11)
            .chain(repeat(RoomListEntryEasy::Empty).take(9))
            .collect::<Vec<_>>()
    );

    let _signal = list.room_list_stream();

    // let's move the window

    list.set_range(1..=10).unwrap();
    // Ensure 0-0 invalidation ranges work.

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "sliding") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Invalid)
            .take(1)
            .chain(repeat(RoomListEntryEasy::Filled).take(10))
            .chain(repeat(RoomListEntryEasy::Empty).take(9))
            .collect::<Vec<_>>()
    );

    list.set_range(5..=10).unwrap();

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "sliding") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Invalid)
            .take(5)
            .chain(repeat(RoomListEntryEasy::Filled).take(6))
            .chain(repeat(RoomListEntryEasy::Empty).take(9))
            .collect::<Vec<_>>()
    );

    // let's move the window

    list.set_range(5..=15).unwrap();

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "sliding") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Invalid)
            .take(5)
            .chain(repeat(RoomListEntryEasy::Filled).take(11))
            .chain(repeat(RoomListEntryEasy::Empty).take(4))
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn moving_out_of_sliding_window() -> anyhow::Result<()> {
    let (client, sync_proxy_builder) = random_setup_with_rooms(20).await?;
    let sliding_window_list = SlidingSyncList::builder("sliding")
        .sync_mode(SlidingSyncMode::Selective)
        .set_range(1..=10)
        .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
        .build();
    let sync_proxy = sync_proxy_builder.add_list(sliding_window_list).build().await?;
    let list = sync_proxy.list("sliding").context("but we just added that list!")?;
    let stream = sync_proxy.stream();
    pin_mut!(stream);
    let room_summary =
        stream.next().await.context("No room summary found, loop ended unsuccessfully")?;
    let summary = room_summary?;
    // we only heard about the ones we had asked for
    assert_eq!(summary.rooms.len(), 10);
    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Empty)
            .take(1)
            .chain(repeat(RoomListEntryEasy::Filled).take(10))
            .chain(repeat(RoomListEntryEasy::Empty).take(9))
            .collect::<Vec<_>>()
    );

    let _signal = list.room_list_stream();

    // let's move the window

    list.set_range(0..=10).unwrap();

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "sliding") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Filled)
            .take(11)
            .chain(repeat(RoomListEntryEasy::Empty).take(9))
            .collect::<Vec<_>>()
    );

    // let's move the window again

    list.set_range(2..=12).unwrap();

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "sliding") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Invalid)
            .take(2)
            .chain(repeat(RoomListEntryEasy::Filled).take(11))
            .chain(repeat(RoomListEntryEasy::Empty).take(7))
            .collect::<Vec<_>>()
    );

    // now we "move" the room of pos 3 to pos 0;
    // this is a bordering case

    let room_id = assert_matches!(list.room_list().get(3), Some(RoomListEntry::Filled(room_id)) => room_id.clone());

    let room = client.get_joined_room(&room_id).context("No joined room {room_id}")?;

    let content = RoomMessageEventContent::text_plain("Hello world");

    room.send(content, None).await?; // this should put our room up to the most recent

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "sliding") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Invalid)
            .take(2)
            .chain(repeat(RoomListEntryEasy::Filled).take(11))
            .chain(repeat(RoomListEntryEasy::Empty).take(7))
            .collect::<Vec<_>>()
    );

    // items has moved, thus we shouldn't find it where it was
    assert!(list.room_list::<RoomListEntry>().get(3).unwrap().as_room_id().unwrap() != room_id);

    // let's move the window again

    list.set_range(0..=10).unwrap();

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "sliding") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Filled)
            .take(11)
            .chain(repeat(RoomListEntryEasy::Invalid).take(2))
            .chain(repeat(RoomListEntryEasy::Empty).take(7))
            .collect::<Vec<_>>()
    );

    // and check that our room move has been accepted properly, too.
    assert_eq!(list.room_list::<RoomListEntry>().get(0).unwrap().as_room_id().unwrap(), &room_id);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "this is a slow test about cold cache recovery"]
async fn fast_unfreeze() -> anyhow::Result<()> {
    let (_client, sync_proxy_builder) = random_setup_with_rooms(500).await?;
    print!("setup took its time");
    let build_lists = || {
        let sliding_window_list = SlidingSyncList::builder("sliding")
            .sync_mode(SlidingSyncMode::Selective)
            .set_range(1..=10)
            .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
            .build();
        let growing_sync = SlidingSyncList::builder("growing")
            .sync_mode(SlidingSyncMode::Growing)
            .full_sync_maximum_number_of_rooms_to_fetch(100)
            .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
            .build();
        anyhow::Ok((sliding_window_list, growing_sync))
    };

    println!("starting the sliding sync setup");

    {
        // SETUP
        let (sliding_window_list, growing_sync) = build_lists()?;
        let sync_proxy = sync_proxy_builder
            .clone()
            .storage_key(Some("sliding_sync".to_owned()))
            .add_list(sliding_window_list)
            .add_list(growing_sync)
            .build()
            .await?;
        let growing_sync = sync_proxy.list("growing").context("but we just added that list!")?; // let's catch it up fully.
        let stream = sync_proxy.stream();
        pin_mut!(stream);
        while growing_sync.state() != SlidingSyncState::FullyLoaded {
            // we wait until growing sync is all done, too
            println!("awaiting");
            let _room_summary = stream
                .next()
                .await
                .context("No room summary found, loop ended unsuccessfully")??;
        }
    }

    println!("starting from cold");
    // recover from frozen state.
    let (sliding_window_list, growing_sync) = build_lists()?;
    // we recover only the window. this should be quick!

    let start = Instant::now();
    let _sync_proxy = sync_proxy_builder
        .clone()
        .storage_key(Some("sliding_sync".to_owned()))
        .add_list(sliding_window_list)
        .add_list(growing_sync)
        .build()
        .await?;
    let duration = start.elapsed();

    assert!(duration < Duration::from_micros(10), "cold recovery was too slow: {duration:?}");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn growing_sync_keeps_going() -> anyhow::Result<()> {
    let (_client, sync_proxy_builder) = random_setup_with_rooms(20).await?;
    let growing_sync = SlidingSyncList::builder("growing")
        .sync_mode(SlidingSyncMode::Growing)
        .full_sync_batch_size(5)
        .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
        .build();

    let sync_proxy = sync_proxy_builder.clone().add_list(growing_sync).build().await?;
    let list = sync_proxy.list("growing").context("but we just added that list!")?;

    let stream = sync_proxy.stream();
    pin_mut!(stream);

    // we have 20 and catch up in batches of 5. so let's get over to 15.

    for _ in 0..=2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let _summary = room_summary?;
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Filled)
            .take(15)
            .chain(repeat(RoomListEntryEasy::Empty).take(5))
            .collect::<Vec<_>>()
    );

    // we have 20 and catch up in batches of 5. let's go one more, see it grows.
    let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
    let _summary = room_summary?;

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(collection_simple, repeat(RoomListEntryEasy::Filled).take(20).collect::<Vec<_>>());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn continue_on_reset() -> anyhow::Result<()> {
    let (_client, sync_proxy_builder) = random_setup_with_rooms(10).await?;
    print!("setup took its time");
    let growing_sync = SlidingSyncList::builder("growing")
        .sync_mode(SlidingSyncMode::Growing)
        .full_sync_batch_size(5)
        .full_sync_maximum_number_of_rooms_to_fetch(100)
        .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
        .build();

    println!("starting the sliding sync setup");
    let sync_proxy = sync_proxy_builder
        .clone()
        .storage_key(Some("sliding_sync".to_owned()))
        .add_list(growing_sync)
        .build()
        .await?;
    let list = sync_proxy.list("growing").context("but we just added that list!")?; // let's catch it up fully.
    let stream = sync_proxy.stream();
    pin_mut!(stream);

    for _ in 0..=2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;

        if summary.lists.iter().any(|s| s == "growing") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple.iter().fold(0, |acc, i| if *i == RoomListEntryEasy::Filled {
            acc + 1
        } else {
            acc
        }),
        5
    );

    // force the pos to be invalid and thus this being reset internally
    sync_proxy.set_pos("100".to_owned());
    let mut error_seen = false;

    for _ in 0..2 {
        let summary = match stream.next().await {
            Some(Ok(e)) => e,
            Some(Err(e)) => {
                match e.client_api_error_kind() {
                    Some(RumaError::UnknownPos) => {
                        // we expect this to come through.
                        error_seen = true;
                        continue;
                    }
                    _ => Err(e)?,
                }
            }
            None => anyhow::bail!("Stream ended unexpectedly."),
        };

        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "growing") {
            break;
        }
    }

    assert!(error_seen, "We have not seen the UnknownPos error");

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple.iter().fold(0, |acc, i| if *i == RoomListEntryEasy::Filled {
            acc + 1
        } else {
            acc
        }),
        10
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn noticing_new_rooms_in_growing() -> anyhow::Result<()> {
    let (client, sync_proxy_builder) = random_setup_with_rooms(20).await?;
    print!("setup took its time");
    let growing_sync = SlidingSyncList::builder("growing")
        .sync_mode(SlidingSyncMode::Growing)
        .full_sync_batch_size(10)
        .full_sync_maximum_number_of_rooms_to_fetch(100)
        .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
        .build();

    println!("starting the sliding sync setup");
    let sync_proxy = sync_proxy_builder
        .clone()
        .storage_key(Some("sliding_sync".to_owned()))
        .add_list(growing_sync)
        .build()
        .await?;
    let list = sync_proxy.list("growing").context("but we just added that list!")?; // let's catch it up fully.
    let stream = sync_proxy.stream();
    pin_mut!(stream);
    while list.state() != SlidingSyncState::FullyLoaded {
        // we wait until growing sync is all done, too
        println!("awaiting");
        let _room_summary =
            stream.next().await.context("No room summary found, loop ended unsuccessfully")??;
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple.iter().fold(0, |acc, i| if *i == RoomListEntryEasy::Filled {
            acc + 1
        } else {
            acc
        }),
        20
    );
    // all found. let's add two more.

    make_room(&client, "one-more".to_owned()).await?;
    make_room(&client, "two-more".to_owned()).await?;

    let mut seen = false;

    for _n in 0..4 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")?;
        let summary = room_summary?;
        // we only heard about the ones we had asked for
        if summary.lists.iter().any(|s| s == "growing")
            && list.maximum_number_of_rooms().unwrap_or_default() == 22
        {
            if seen {
                // once we saw 22, we give it another loop to catch up!
                break;
            } else {
                seen = true;
            }
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple.iter().fold(0, |acc, i| if *i == RoomListEntryEasy::Filled {
            acc + 1
        } else {
            acc
        }),
        22
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restart_room_resubscription() -> anyhow::Result<()> {
    let (client, sync_proxy_builder) = random_setup_with_rooms(3).await?;

    let sync_proxy = sync_proxy_builder
        .add_list(
            SlidingSyncList::builder("sliding_list")
                .sync_mode(SlidingSyncMode::Selective)
                .set_range(0..=2)
                .sort(vec!["by_recency".to_owned(), "by_name".to_owned()])
                .build(),
        )
        .build()
        .await?;

    let list = sync_proxy.list("sliding_list").context("list `sliding_list` isn't found")?;

    let stream = sync_proxy.stream();
    pin_mut!(stream);

    let room_summary =
        stream.next().await.context("No room summary found, loop ended unsuccessfully")??;

    // we only heard about the ones we had asked for
    assert_eq!(room_summary.rooms.len(), 3);

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(collection_simple, repeat(RoomListEntryEasy::Filled).take(3).collect::<Vec<_>>());

    let _signal = list.room_list_stream();

    // let's move the window

    list.set_range(1..=2).unwrap();

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")??;

        // we only heard about the ones we had asked for
        if room_summary.lists.iter().any(|s| s == "sliding_list") {
            break;
        }
    }

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Invalid)
            .take(1)
            .chain(repeat(RoomListEntryEasy::Filled).take(2))
            .collect::<Vec<_>>()
    );

    // let's get that first entry

    let room_id = assert_matches!(list.room_list().get(0), Some(RoomListEntry::Invalidated(room_id)) => room_id.clone());

    // send a message

    let room = client.get_joined_room(&room_id).context("No joined room {room_id}")?;

    let content = RoomMessageEventContent::text_plain("Hello world");

    room.send(content, None).await?; // this should put our room up to the most recent

    // let's subscribe

    sync_proxy.subscribe(room_id.clone(), Default::default());

    let mut room_updated = false;

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")??;

        // we only heard about the ones we had asked for
        if room_summary.rooms.iter().any(|s| s == &room_id) {
            room_updated = true;
            break;
        }
    }

    assert!(room_updated, "Room update has not been seen");

    // force the pos to be invalid and thus this being reset internally
    sync_proxy.set_pos("100".to_owned());

    let mut error_seen = false;
    let mut room_updated = false;

    for _n in 0..2 {
        let summary = match stream.next().await {
            Some(Ok(e)) => e,
            Some(Err(e)) => {
                match e.client_api_error_kind() {
                    Some(RumaError::UnknownPos) => {
                        // we expect this to come through.
                        error_seen = true;
                        continue;
                    }
                    _ => Err(e)?,
                }
            }
            None => anyhow::bail!("Stream ended unexpectedly."),
        };

        // we only heard about the ones we had asked for
        if summary.rooms.iter().any(|s| s == &room_id) {
            room_updated = true;
            break;
        }
    }

    assert!(error_seen, "We have not seen the UnknownPos error");
    assert!(room_updated, "Room update has not been seen");

    // send another message

    let room = client.get_joined_room(&room_id).context("No joined room {room_id}")?;

    let content = RoomMessageEventContent::text_plain("Hello world");

    let event_id = room.send(content, None).await?.event_id; // this should put our room up to the most recent

    // let's see for it to come down the pipe
    let mut room_updated = false;

    for _n in 0..2 {
        let room_summary = stream.next().await.context("sync has closed unexpectedly")??;

        // we only heard about the ones we had asked for
        if room_summary.rooms.iter().any(|s| s == &room_id) {
            room_updated = true;
            break;
        }
    }
    assert!(room_updated, "Room update has not been seen");

    let sliding_sync_room = sync_proxy.get_room(&room_id).expect("Slidin Sync room not found");
    let event = sliding_sync_room.latest_event().await.expect("No even found");

    let collection_simple = list.room_list::<RoomListEntryEasy>();

    assert_eq!(
        collection_simple,
        repeat(RoomListEntryEasy::Invalid)
            .take(1)
            .chain(repeat(RoomListEntryEasy::Filled).take(2))
            .collect::<Vec<_>>()
    );

    assert_eq!(
        event.event_id().unwrap(),
        event_id,
        "Latest event is different than what we've sent"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn receipts_extension_works() -> anyhow::Result<()> {
    let (client, sync_proxy_builder) = random_setup_with_rooms(1).await?;
    let list = SlidingSyncList::builder("a")
        .sync_mode(SlidingSyncMode::Selective)
        .ranges(vec![(0..=1)])
        .sort(vec!["by_recency".to_owned()])
        .build();

    let mut config = ReceiptsConfig::default();
    config.enabled = Some(true);

    let sync_proxy =
        sync_proxy_builder.clone().add_list(list).with_receipt_extension(config).build().await?;
    let list = sync_proxy.list("a").context("but we just added that list!")?;

    let stream = sync_proxy.stream();
    pin_mut!(stream);

    stream.next().await.context("sync has closed unexpectedly")??;

    // find the room and send an event which we will send a receipt for
    let room_id = list.get_room_id(0).unwrap();
    let room = client.get_joined_room(&room_id).context("No joined room {room_id}")?;
    let event_id =
        room.send(RoomMessageEventContent::text_plain("Hello world"), None).await?.event_id;

    // now send a receipt
    room.send_single_receipt(CreateReceiptType::Read, ReceiptThread::Unthreaded, event_id.clone())
        .await?;

    // we expect to see it because we have enabled the receipt extension. We don't
    // know when we'll see it though
    let mut found_receipt = false;
    for _n in 0..10 {
        stream.next().await.context("sync has closed unexpectedly")??;

        // try to find it
        let room = client.get_room(&room_id).context("No joined room {room_id}")?;
        let receipts = room
            .event_receipts(ReceiptType::Read, ReceiptThread::Unthreaded, &event_id)
            .await
            .unwrap();

        let expected_user_id = client.user_id().unwrap();
        found_receipt = receipts.iter().any(|(user_id, _)| user_id == expected_user_id);
        if found_receipt {
            break;
        }
    }
    assert!(found_receipt);
    Ok(())
}
*/
