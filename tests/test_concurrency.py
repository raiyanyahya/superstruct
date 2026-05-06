"""Thread safety tests."""
import threading
import unittest

from superstruct import Superstruct


class ConcurrencyTests(unittest.TestCase):
    def test_simultaneous_inserts_and_queries_do_not_corrupt(self):
        ss = Superstruct()

        def writer():
            for i in range(500):
                ss.insert({"city": "NYC", "n": i})

        def reader():
            for _ in range(500):
                # Result count grows over time but the call must never
                # raise an exception due to concurrent mutation.
                ss.find().equals("city", "NYC").execute()

        threads = [
            threading.Thread(target=writer),
            threading.Thread(target=writer),
            threading.Thread(target=reader),
            threading.Thread(target=reader),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        self.assertEqual(len(ss), 1000)
        out = ss.find().equals("city", "NYC").execute()
        self.assertEqual(len(out), 1000)

    def test_simultaneous_inserts_and_deletes_stay_consistent(self):
        ss = Superstruct()
        ids: list[int] = []
        ids_lock = threading.Lock()

        def inserter():
            for i in range(200):
                rid = ss.insert({"n": i})
                with ids_lock:
                    ids.append(rid)

        def deleter():
            # Wait until inserts are happening then start deleting.
            for _ in range(200):
                with ids_lock:
                    rid = ids.pop() if ids else None
                if rid is not None:
                    ss.delete(rid)

        ts = [
            threading.Thread(target=inserter),
            threading.Thread(target=inserter),
            threading.Thread(target=deleter),
        ]
        for t in ts:
            t.start()
        for t in ts:
            t.join()

        # The result count over a wide range should equal the live store.
        out = ss.find().range("n", -1, 999).execute()
        self.assertEqual(len(out), len(ss))

    def test_thread_safe_off_works_in_single_thread(self):
        ss = Superstruct(thread_safe=False)
        ss.insert({"x": 1})
        out = ss.find().equals("x", 1).execute()
        self.assertEqual(len(out), 1)


if __name__ == "__main__":
    unittest.main()
