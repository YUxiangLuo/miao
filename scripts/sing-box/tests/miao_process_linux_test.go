//go:build linux

package process

import (
	"errors"
	"net"
	"os"
	"reflect"
	"strconv"
	"sync"
	"syscall"
	"testing"
	"testing/synctest"
	"time"

	E "github.com/sagernet/sing/common/exceptions"
)

func miaoProcessSearcher(t testing.TB) *linuxSearcher {
	t.Helper()
	searcher, err := NewSearcher(Config{})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { searcher.Close() })
	return searcher.(*linuxSearcher)
}

func miaoSocketInode(t testing.TB) uint32 {
	t.Helper()
	conn, err := net.ListenUDP("udp4", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { conn.Close() })
	raw, err := conn.SyscallConn()
	if err != nil {
		t.Fatal(err)
	}
	var stat syscall.Stat_t
	var statErr error
	if err := raw.Control(func(fd uintptr) { statErr = syscall.Fstat(int(fd), &stat) }); err != nil {
		t.Fatal(err)
	}
	if statErr != nil {
		t.Fatal(statErr)
	}
	if stat.Ino > uint64(^uint32(0)) {
		t.Fatal("socket inode exceeds upstream uint32 representation")
	}
	return uint32(stat.Ino)
}

// Hold a completed scan before singleflight publishes it. synctest.Wait ensures
// callers have joined the flight, without scheduler sleeps or production hooks.
func miaoHoldProcessScan(t *testing.T, s *linuxSearcher, uid uint32, paths map[uint32][]string, err error) func() {
	t.Helper()
	entered, release := make(chan struct{}), make(chan struct{})
	go func() {
		s.processPathScans.Do(strconv.FormatUint(uint64(uid), 10), func() (any, error) {
			close(entered)
			<-release
			return paths, err
		})
	}()
	<-entered
	return func() { close(release) }
}

func TestMiaoProcessSharedPositive(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		s := miaoProcessSearcher(t)
		const inode, uid, callers = 77, 1234, 32
		want := []string{"/fixture/process-a", "/fixture/process-b"}
		release := miaoHoldProcessScan(t, s, uid, map[uint32][]string{inode: want}, nil)
		results := make(chan []string, callers)
		for range callers {
			go func() { paths, _ := s.findProcessPaths(inode, uid); results <- paths }()
		}
		synctest.Wait()
		if len(results) != 0 {
			t.Fatal("lookup did not wait for shared scan")
		}
		release()
		for range callers {
			if got := <-results; !reflect.DeepEqual(got, want) {
				t.Fatalf("shared attribution: %v", got)
			}
		}
	})
}

func TestMiaoProcessNewSocketDuringScan(t *testing.T) {
	for _, tc := range []struct {
		name             string
		otherUID, failed bool
	}{
		{"same-uid", false, false}, {"all-users-fallback", true, false}, {"shared-error", false, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				s := miaoProcessSearcher(t)
				uid := uint32(os.Getuid())
				if tc.otherUID {
					uid++
				}
				// Capture actual /proc contents before opening the new socket.
				oldPaths, err := buildProcessPaths(uid)
				if err != nil {
					t.Fatal(err)
				}
				oldAll, err := buildProcessPaths(processPathsAllUsers)
				if err != nil {
					t.Fatal(err)
				}
				var scanErr error
				if tc.failed {
					scanErr = errors.New("earlier caller's scan failed")
				}
				releaseUID := miaoHoldProcessScan(t, s, uid, oldPaths, scanErr)
				releaseAll := miaoHoldProcessScan(t, s, processPathsAllUsers, oldAll, nil)
				inode := miaoSocketInode(t)
				if _, exists := oldAll[inode]; exists {
					t.Fatal("fixture reused an existing inode")
				}
				type result struct {
					paths []string
					err   error
				}
				done := make(chan result, 1)
				go func() { paths, err := s.findProcessPaths(inode, uid); done <- result{paths, err} }()
				synctest.Wait()
				if len(done) != 0 {
					t.Fatal("lookup did not join UID scan")
				}
				releaseUID()
				synctest.Wait()
				if tc.otherUID && len(done) != 0 {
					t.Fatal("lookup skipped all-user fallback")
				}
				releaseAll()
				got := <-done
				// /proc/self/exe is independent of the lookup's directory scan.
				want, err := os.Readlink("/proc/self/exe")
				if err != nil {
					t.Fatal(err)
				}
				if got.err != nil || !reflect.DeepEqual(got.paths, []string{want}) {
					t.Fatalf("new socket: %v, %v; want %s", got.paths, got.err, want)
				}
			})
		})
	}
}

func TestMiaoProcessCacheBoundaries(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		s := miaoProcessSearcher(t)
		uid, inode := uint32(os.Getuid()), miaoSocketInode(t)
		want, err := os.Readlink("/proc/self/exe")
		if err != nil {
			t.Fatal(err)
		}
		for _, reset := range []bool{false, true} {
			s.processPathCache.Add(uid, &uidProcessPaths{entries: map[uint32][]string{inode: {"cached"}}})
			if paths, err := s.findProcessPaths(inode, uid); err != nil || !reflect.DeepEqual(paths, []string{"cached"}) {
				t.Fatalf("cache hit: %v, %v", paths, err)
			}
			if reset {
				s.ResetCache()
			} else {
				time.Sleep(1100 * time.Millisecond)
			}
			if paths, err := s.findProcessPaths(inode, uid); err != nil || !reflect.DeepEqual(paths, []string{want}) {
				t.Fatalf("cache refresh: %v, %v", paths, err)
			}
		}
		if _, err := s.findProcessPaths(0, uid); err == nil {
			t.Fatal("missing inode must report failure")
		}
		newInode := miaoSocketInode(t)
		if paths, err := s.findProcessPaths(newInode, uid); err != nil || !reflect.DeepEqual(paths, []string{want}) {
			t.Fatalf("missing inode was negatively cached: %v, %v", paths, err)
		}
	})
}

// Frozen upstream implementation at 474c63c3, retained only for paired benchmarks.
func miaoOriginalProcessPaths(s *linuxSearcher, targetInode, uid uint32) ([]string, error) {
	for _, scanUID := range []uint32{uid, processPathsAllUsers} {
		if cached, ok := s.processPathCache.Get(scanUID); ok {
			if paths, found := cached.entries[targetInode]; found {
				return paths, nil
			}
		}
		paths, err := buildProcessPaths(scanUID)
		if err != nil {
			return nil, err
		}
		s.processPathCache.Add(scanUID, &uidProcessPaths{entries: paths})
		if paths, found := paths[targetInode]; found {
			return paths, nil
		}
	}
	return nil, E.New("process of uid(", uid, "), inode(", targetInode, ") not found")
}

func BenchmarkMiaoProcessPaths(b *testing.B) {
	uid := uint32(os.Getuid())
	for _, workload := range []struct {
		name          string
		workers       int
		warm, missing bool
	}{
		{"warm", 1, true, false}, {"cold", 1, false, false}, {"burst32", 32, false, false}, {"missing32", 32, false, true},
	} {
		b.Run(workload.name, func(b *testing.B) {
			for _, variant := range []struct {
				name string
				find func(*linuxSearcher, uint32, uint32) ([]string, error)
			}{
				{"upstream", miaoOriginalProcessPaths}, {"coalesced", (*linuxSearcher).findProcessPaths},
			} {
				b.Run(variant.name, func(b *testing.B) {
					s := miaoProcessSearcher(b)
					inodes := make([]uint32, workload.workers)
					for i := range inodes {
						if !workload.missing {
							inodes[i] = miaoSocketInode(b)
						}
					}
					if workload.warm {
						if _, err := variant.find(s, inodes[0], uid); err != nil {
							b.Fatal(err)
						}
					}
					want, err := os.Readlink("/proc/self/exe")
					if err != nil {
						b.Fatal(err)
					}
					lookup := func(i int) {
						paths, err := variant.find(s, inodes[i], uid)
						if workload.missing {
							if err == nil {
								b.Error("missing inode accepted")
							}
						} else if err != nil || len(paths) != 1 || paths[0] != want {
							b.Errorf("attribution lost: %v, %v", paths, err)
						}
					}
					b.ReportAllocs()
					b.ResetTimer()
					for range b.N {
						if !workload.warm {
							s.ResetCache()
						}
						if workload.workers == 1 {
							lookup(0)
							continue
						}
						start := make(chan struct{})
						var wg sync.WaitGroup
						for i := range inodes {
							wg.Go(func() { <-start; lookup(i) })
						}
						close(start)
						wg.Wait()
					}
				})
			}
		})
	}
}
